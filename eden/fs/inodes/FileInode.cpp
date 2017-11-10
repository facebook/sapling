/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/FileInode.h"

#include <folly/FileUtil.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <openssl/sha.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileHandle.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/XAttr.h"

using folly::ByteRange;
using folly::Future;
using folly::StringPiece;
using folly::Unit;
using folly::checkUnixError;
using folly::makeFuture;
using std::shared_ptr;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

FileInode::State::State(
    FileInode* inode,
    mode_t m,
    const folly::Optional<Hash>& h,
    const timespec& lastCheckoutTime)
    : mode(m), hash(h) {
  if (!h.hasValue()) {
    // File is materialized
    auto filePath = inode->getLocalPath();
    file = Overlay::openFile(
        filePath.c_str(), Overlay::kHeaderIdentifierFile, timeStamps);
  } else {
    timeStamps.setTimestampValues(lastCheckoutTime);
  }

  checkInvariants();
}

FileInode::State::State(
    FileInode* inode,
    mode_t m,
    folly::File&& file,
    const timespec& lastCheckoutTime,
    dev_t rdev)
    : mode(m), rdev(rdev), file(std::move(file)) {
  timeStamps.setTimestampValues(lastCheckoutTime);
  checkInvariants();
}

void FileInode::State::State::checkInvariants() {
  if (blob) {
    // 'loaded'
    CHECK(hash);
    CHECK(!file);
    CHECK(!sha1Valid);
  } else if (file) {
    // 'materialized'
    CHECK(!hash);
    CHECK(!blob);
    CHECK(file);
  } else {
    // 'not loaded'
    CHECK(hash);
    CHECK(!blob);
    CHECK(!file);
    CHECK(!sha1Valid);
  }
}
/*
 * Defined State Destructor explicitly to avoid including
 * some header files in FileInode.h
 */
FileInode::State::~State() = default;

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    const folly::Optional<Hash>& hash)
    : InodeBase(ino, std::move(parentInode), name),
      state_(
          folly::in_place,
          this,
          mode,
          hash,
          getMount()->getLastCheckoutTime()) {}

FileInode::FileInode(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    folly::File&& file,
    dev_t rdev)
    : InodeBase(ino, std::move(parentInode), name),
      state_(
          folly::in_place,
          this,
          mode,
          std::move(file),
          getMount()->getLastCheckoutTime(),
          rdev) {}

folly::Future<fusell::Dispatcher::Attr> FileInode::getattr() {
  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  return stat().then(
      [nodeId = getNodeId(),
       initStat = getMount()->initStatData()](const struct stat& st) {
        auto attr = fusell::Dispatcher::Attr{initStat};
        attr.st = st;
        attr.st.st_ino = nodeId;
        return attr;
      });
}

folly::Future<fusell::Dispatcher::Attr> FileInode::setInodeAttr(
    const struct stat& attr,
    int to_set) {
  int openFlags = O_RDWR;

  // Minor optimization: if we know that the file is being completed truncated
  // as part of this operation, there's no need to fetch the underlying data,
  // so pass on the truncate flag our underlying open call
  if ((to_set & FUSE_SET_ATTR_SIZE) && attr.st_size == 0) {
    openFlags |= O_TRUNC;
  }

  return materializeForWrite(openFlags).then(
      [self = inodePtrFromThis(), attr, to_set]() {
        self->materializeInParent();

        auto result =
            fusell::Dispatcher::Attr{self->getMount()->initStatData()};

        auto state = self->state_.wlock();
        CHECK(state->file) << "MUST have a materialized file at this point";

        // Set the size of the file when FUSE_SET_ATTR_SIZE is set
        if (to_set & FUSE_SET_ATTR_SIZE) {
          checkUnixError(ftruncate(
              state->file.fd(), attr.st_size + Overlay::kHeaderLength));
        }

        if (to_set & FUSE_SET_ATTR_MODE) {
          // The mode data is stored only in inode_->state_.
          // (We don't set mode bits on the overlay file as that may incorrectly
          // prevent us from reading or writing the overlay data).
          // Make sure we preserve the file type bits, and only update
          // permissions.
          state->mode = (state->mode & S_IFMT) | (07777 & attr.st_mode);
        }

        // Set in-memory timeStamps
        self->setattrTimes(attr, to_set, state->timeStamps);

        // We need to call fstat function here to get the size of the overlay
        // file. We might update size in the result while truncating the file
        // when FUSE_SET_ATTR_SIZE flag is set but when the flag is not set we
        // have to return the correct size of the file even if some size is sent
        // in attr.st.st_size.
        checkUnixError(fstat(state->file.fd(), &result.st));
        result.st.st_ino = self->getNodeId();
        result.st.st_size -= Overlay::kHeaderLength;
        result.st.st_atim = state->timeStamps.atime;
        result.st.st_ctim = state->timeStamps.ctime;
        result.st.st_mtim = state->timeStamps.mtime;
        result.st.st_mode = state->mode;

        state->checkInvariants();

        // Update the Journal
        self->updateJournal();
        return result;
      });
}

folly::Future<std::string> FileInode::readlink() {
  {
    // TODO: Since the type component of the mode is immutable, it could be
    // moved out of the locked state, obviating the need to acquire a lock
    // here.
    auto state = state_.rlock();
    if (!S_ISLNK(state->mode)) {
      // man 2 readlink says:  EINVAL The named file is not a symbolic link.
      throw InodeError(EINVAL, inodePtrFromThis(), "not a symlink");
    }
  }

  // The symlink contents are simply the file contents!
  return readAll();
}

void FileInode::fileHandleDidClose() {
  {
    // TODO(T20329170): We might need this function in the Future if we decide
    // to write in memory timestamps to overlay file on
    // file handle close.
  }
}

AbsolutePath FileInode::getLocalPath() const {
  return getMount()->getOverlay()->getFilePath(getNodeId());
}

folly::Optional<bool> FileInode::isSameAsFast(const Hash& blobID, mode_t mode) {
  // When comparing mode bits, we only care about the
  // file type and owner permissions.
  auto relevantModeBits = [](mode_t m) { return (m & (S_IFMT | S_IRWXU)); };

  auto state = state_.rlock();
  if (relevantModeBits(state->mode) != relevantModeBits(mode)) {
    return false;
  }

  if (state->hash.hasValue()) {
    // This file is not materialized, so we can just compare hashes
    return state->hash.value() == blobID;
  }
  return folly::none;
}

bool FileInode::isSameAs(const Blob& blob, mode_t mode) {
  auto result = isSameAsFast(blob.getHash(), mode);
  if (result.hasValue()) {
    return result.value();
  }

  return getSha1().value() == Hash::sha1(&blob.getContents());
}

folly::Future<bool> FileInode::isSameAs(const Hash& blobID, mode_t mode) {
  auto result = isSameAsFast(blobID, mode);
  if (result.hasValue()) {
    return makeFuture(result.value());
  }

  return getMount()->getObjectStore()->getBlobMetadata(blobID).then(
      [self = inodePtrFromThis()](const BlobMetadata& metadata) {
        return self->getSha1().value() == metadata.sha1;
      });
}

mode_t FileInode::getMode() const {
  return state_.rlock()->mode;
}

mode_t FileInode::getPermissions() const {
  return (getMode() & 07777);
}

folly::Optional<Hash> FileInode::getBlobHash() const {
  return state_.rlock()->hash;
}

folly::Future<std::shared_ptr<fusell::FileHandle>> FileInode::open(
    const struct fuse_file_info& fi) {
// TODO: We currently should ideally call fileHandleDidClose() if we fail
// to create a FileHandle.  It's currently slightly tricky to do this right
// on all code paths.
//
// I think it will be better in the long run to just refactor how we do this.
// fileHandleDidClose() currently uses std::shared_ptr::unique(), which is
// deprecated in future versions of C++.
#if 0
  SCOPE_EXIT {
    fileHandleDidClose();
  };
#endif

  {
    // TODO: Since the type component of the mode is immutable, it could be
    // moved out of the locked state, obviating the need to acquire a lock
    // here.
    auto state = state_.rlock();

    if (S_ISLNK(state->mode)) {
      // Linux reports ELOOP if you try to open a symlink with O_NOFOLLOW set.
      // Since it isn't clear whether FUSE will allow this to happen, this
      // is a speculative defense against that happening; the O_PATH flag
      // does allow a file handle to be opened on a symlink on Linux,
      // but does not allow it to be used for real IO operations.  We're
      // punting on handling those situations here for now.
      throw InodeError(ELOOP, inodePtrFromThis(), "is a symlink");
    }
  }

  if (fi.flags & (O_RDWR | O_WRONLY | O_CREAT | O_TRUNC)) {
    return materializeForWrite(fi.flags).then([self = inodePtrFromThis()]() {
      self->materializeInParent();
      return shared_ptr<fusell::FileHandle>{std::make_shared<FileHandle>(self)};
    });
  } else {
    return ensureDataLoaded().then([self = inodePtrFromThis()]() {
      return shared_ptr<fusell::FileHandle>{std::make_shared<FileHandle>(self)};
    });
  }
}

void FileInode::materializeInParent() {
  auto renameLock = getMount()->acquireRenameLock();
  auto loc = getLocationInfo(renameLock);
  if (loc.parent && !loc.unlinked) {
    loc.parent->childMaterialized(renameLock, loc.name, getNodeId());
  }
}

std::shared_ptr<FileHandle> FileInode::finishCreate() {
  SCOPE_EXIT {
    fileHandleDidClose();
  };
  return std::make_shared<FileHandle>(inodePtrFromThis());
}

Future<vector<string>> FileInode::listxattr() {
  // Currently, we only return a non-empty vector for regular files, and we
  // assume that the SHA-1 is present without checking the ObjectStore.
  vector<string> attributes;

  {
    auto state = state_.rlock();
    if (S_ISREG(state->mode)) {
      attributes.emplace_back(kXattrSha1.str());
    }
  }
  return attributes;
}

Future<string> FileInode::getxattr(StringPiece name) {
  // Currently, we only support the xattr for the SHA-1 of a regular file.
  if (name != kXattrSha1) {
    return makeFuture<string>(InodeError(kENOATTR, inodePtrFromThis()));
  }

  return getSha1().then([](Hash hash) { return hash.toString(); });
}

Future<Hash> FileInode::getSha1(bool failIfSymlink) {
  auto state = state_.wlock();
  state->checkInvariants();

  if (failIfSymlink && !S_ISREG(state->mode)) {
    // We only define a SHA-1 value for regular files
    return makeFuture<Hash>(InodeError(kENOATTR, inodePtrFromThis()));
  }

  if (state->hash.hasValue()) {
    // If a file is not materialized it should have a hash value.
    return getObjectStore()
        ->getBlobMetadata(state->hash.value())
        .then([](const BlobMetadata& metadata) { return metadata.sha1; });
  } else if (state->file) {
    // If the file is materialized.
    if (state->sha1Valid) {
      auto shaStr = fgetxattr(state->file.fd(), kXattrSha1);
      if (!shaStr.empty()) {
        return Hash(shaStr);
      }
    }
    return recomputeAndStoreSha1(state);
  } else {
    auto bug = EDEN_BUG()
        << "One of state->hash and state->file must be set for the Inode :: "
        << getNodeId() << " :Blob is " << (state->blob ? "not " : "") << "Null";
    return folly::makeFuture<Hash>(bug.toException());
  }
}

folly::Future<struct stat> FileInode::stat() {
  return ensureDataLoaded().then([self = inodePtrFromThis()]() {
    auto st = self->getMount()->initStatData();
    st.st_nlink = 1;

    auto state = self->state_.rlock();

    if (state->file) {
      // We are calling fstat only to get the size of the file.
      checkUnixError(fstat(state->file.fd(), &st));

      if (st.st_size < Overlay::kHeaderLength) {
        auto filePath = self->getLocalPath();
        EDEN_BUG() << "Overlay file " << filePath
                   << " is too short for header: size=" << st.st_size;
      }
      st.st_size -= Overlay::kHeaderLength;
      st.st_rdev = state->rdev;
    } else {
      CHECK(state->blob);
      auto buf = state->blob->getContents();
      st.st_size = buf.computeChainDataLength();

      // NOTE: we don't set rdev to anything special here because we
      // don't support committing special device nodes.
    }
#if defined(_BSD_SOURCE) || defined(_SVID_SOURCE) || \
    _POSIX_C_SOURCE >= 200809L || _XOPEN_SOURCE >= 700
    st.st_atim = state->timeStamps.atime;
    st.st_ctim = state->timeStamps.ctime;
    st.st_mtim = state->timeStamps.mtime;
#else
    st.st_atime = state->timeStamps.atime.tv_sec;
    st.st_mtime = state->timeStamps.mtime.tv_sec;
    st.st_ctime = state->timeStamps.ctime.tv_sec;
#endif
    st.st_mode = state->mode;

    return st;
  });
}

void FileInode::flush(uint64_t /* lock_owner */) {
  // We have no write buffers, so there is nothing for us to flush,
  // but let's take this opportunity to update the sha1 attribute.
  auto state = state_.wlock();
  if (state->file && !state->sha1Valid) {
    recomputeAndStoreSha1(state);
  }
  state->checkInvariants();
}

void FileInode::fsync(bool datasync) {
  auto state = state_.wlock();
  if (!state->file) {
    // If we don't have an overlay file then we have nothing to sync.
    return;
  }

  auto res =
#ifndef __APPLE__
      datasync ? ::fdatasync(state->file.fd()) :
#endif
               ::fsync(state->file.fd());
  checkUnixError(res);

  // let's take this opportunity to update the sha1 attribute.
  if (!state->sha1Valid) {
    recomputeAndStoreSha1(state);
  }
}

std::unique_ptr<folly::IOBuf> FileInode::readIntoBuffer(
    size_t size,
    off_t off) {
  SCOPE_SUCCESS {
    // We do not want to have a write lock on state_ during the entire io
    // operation(read) for updating in memory timeStamps as it will block
    // concurrent reads. Also, we want to update the timeStamps after the read
    // operation, so in order to achieve the above two, we are using
    // SCOPE_SUCCESS macro here

    // Update atime on read systemcall.
    auto state = state_.wlock();
    clock_gettime(CLOCK_REALTIME, &state->timeStamps.atime);
  };
  auto state = state_.rlock();

  if (state->file) {
    auto buf = folly::IOBuf::createCombined(size);
    auto res = ::pread(
        state->file.fd(),
        buf->writableBuffer(),
        size,
        off + Overlay::kHeaderLength);

    checkUnixError(res);
    buf->append(res);
    return buf;
  }

  auto buf = state->blob->getContents();
  folly::io::Cursor cursor(&buf);

  if (!cursor.canAdvance(off)) {
    // Seek beyond EOF.  Return an empty result.
    return folly::IOBuf::wrapBuffer("", 0);
  }

  cursor.skip(off);

  std::unique_ptr<folly::IOBuf> result;
  cursor.cloneAtMost(result, size);
  return result;
}

folly::Future<std::string> FileInode::readAll() {
  return ensureDataLoaded().then([self = inodePtrFromThis()] {
    // We need to take the wlock instead of the rlock because the lseek() call
    // modifies the file offset of the file descriptor.
    auto state = self->state_.wlock();
    std::string result;
    if (state->file) {
      auto rc = lseek(state->file.fd(), Overlay::kHeaderLength, SEEK_SET);
      folly::checkUnixError(rc, "unable to seek in materialized FileInode");
      folly::readFile(state->file.fd(), result);
    } else {
      const auto& contentsBuf = state->blob->getContents();
      folly::io::Cursor cursor(&contentsBuf);
      result = cursor.readFixedString(contentsBuf.computeChainDataLength());
    }

    // We want to update atime after the read operation.
    clock_gettime(CLOCK_REALTIME, &state->timeStamps.atime);
    return result;
  });
}

fusell::BufVec FileInode::read(size_t size, off_t off) {
  auto buf = readIntoBuffer(size, off);
  return fusell::BufVec(std::move(buf));
}

size_t FileInode::write(fusell::BufVec&& buf, off_t off) {
  auto state = state_.wlock();

  if (!state->file) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  state->sha1Valid = false;
  auto vec = buf.getIov();
  auto xfer = ::pwritev(
      state->file.fd(), vec.data(), vec.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);

  // Update mtime and ctime on write systemcall.
  clock_gettime(CLOCK_REALTIME, &state->timeStamps.mtime);
  state->timeStamps.ctime = state->timeStamps.mtime;

  return xfer;
}

size_t FileInode::write(folly::StringPiece data, off_t off) {
  auto state = state_.wlock();

  if (!state->file) {
    // Not open for write
    folly::throwSystemErrorExplicit(EINVAL);
  }

  state->sha1Valid = false;
  auto xfer = ::pwrite(
      state->file.fd(), data.data(), data.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);

  // Update mtime and ctime on write systemcall.
  clock_gettime(CLOCK_REALTIME, &state->timeStamps.mtime);
  state->timeStamps.ctime = state->timeStamps.mtime;

  return xfer;
}

Future<Unit> FileInode::ensureDataLoaded() {
  auto state = state_.wlock();

  if (!state->hash.hasValue()) {
    // We should always have the file open if we are materialized.
    CHECK(state->file);
    return makeFuture();
  }

  if (state->blob) {
    DCHECK_EQ(state->blob->getHash(), state->hash.value());
    return makeFuture();
  }

  // Load the blob data.
  auto blobFuture = getObjectStore()->getBlob(state->hash.value());

  // TODO: We really should defer this using a Future rather than calling get()
  // here and blocking until the load completes.  However, for that to work we
  // will need to add some extra data tracking whether or not we are already in
  // the process of loading the data.  We need to avoid multiple threads all
  // trying to load the data at the same time.
  //
  // For now doing a blocking load with the inode_->state_ lock held ensures
  // that only one thread can load the data at a time.  It's pretty unfortunate
  // to block with the lock held, though :-(
  state->blob = blobFuture.get();
  state->checkInvariants();
  return makeFuture();
}

Future<Unit> FileInode::materializeForWrite(int openFlags) {
  auto state = state_.wlock();

  SCOPE_SUCCESS {
    state->checkInvariants();
  };

  // If we already have a materialized overlay file then we don't
  // need to do much
  if (state->file) {
    CHECK(!state->hash.hasValue());
    if ((openFlags & O_TRUNC) != 0) {
      // truncating a file that we already have open
      state->sha1Valid = false;
      checkUnixError(ftruncate(state->file.fd(), Overlay::kHeaderLength));
      auto emptySha1 = Hash::sha1(ByteRange{});
      storeSha1(state, emptySha1);
    } else {
      // no truncate option,overlay file contain old header
      // we have to update only header but not contents
    }
    return makeFuture();
  }

  // Add header to the overlay File.
  auto header = Overlay::createHeader(
      Overlay::kHeaderIdentifierFile,
      Overlay::kHeaderVersion,
      state->timeStamps.atime,
      state->timeStamps.ctime,
      state->timeStamps.mtime);
  auto iov = header.getIov();

  // We must not be materialized yet
  CHECK(state->hash.hasValue());

  auto filePath = getLocalPath();

  if ((openFlags & O_TRUNC) != 0) {
    folly::writeFileAtomic(filePath.stringPiece(), iov.data(), iov.size());
    // We don't want to set the in-memory timestamps to the timestamps returned
    // by the below openFile function as we just wrote these timestamps in to
    // overlay using writeFileAtomic.
    InodeTimestamps timeStamps;
    state->file = Overlay::openFile(
        filePath.stringPiece(), Overlay::kHeaderIdentifierFile, timeStamps);
    state->sha1Valid = false;
    storeSha1(state, Hash::sha1(ByteRange{}));
  } else {
    if (!state->blob) {
      // TODO: Load the blob using the non-blocking Future APIs.
      // However, just as in ensureDataLoaded() above we will also need
      // to add a mechanism to wait for already in-progress loads.
      auto blobFuture = getObjectStore()->getBlob(state->hash.value());
      state->blob = blobFuture.get();
    }

    // Write the blob contents out to the overlay
    auto contents = state->blob->getContents().getIov();
    iov.insert(iov.end(), contents.begin(), contents.end());

    folly::writeFileAtomic(
        filePath.stringPiece(), iov.data(), iov.size(), 0600);
    InodeTimestamps timeStamps;
    state->file = Overlay::openFile(
        filePath.stringPiece(), Overlay::kHeaderIdentifierFile, timeStamps);
    state->sha1Valid = false;

    // If we have a SHA-1 from the metadata, apply it to the new file.  This
    // saves us from recomputing it again in the case that something opens the
    // file read/write and closes it without changing it.
    auto metadata = getObjectStore()->getBlobMetadata(state->hash.value());
    if (metadata.isReady()) {
      storeSha1(state, metadata.value().sha1);
    } else {
      // Leave the SHA-1 attribute dirty - it is not very likely that a file
      // will be opened for writing, closed without changing, and then have its
      // SHA-1 queried via Thrift or xattr. If so, the SHA-1 will be recomputed
      // as needed. That said, it's perhaps cheaper to hash now (SHA-1 is
      // hundreds of MB/s) while the data is accessible in the blob than to read
      // the file out of the overlay later.
    }
  }

  // Update the FileInode to indicate that we are materialized now
  state->blob.reset();
  state->hash = folly::none;

  return makeFuture();
}

ObjectStore* FileInode::getObjectStore() const {
  return getMount()->getObjectStore();
}

Hash FileInode::recomputeAndStoreSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state) {
  uint8_t buf[8192];
  off_t off = Overlay::kHeaderLength;
  SHA_CTX ctx;
  SHA1_Init(&ctx);

  while (true) {
    // Using pread here so that we don't move the file position;
    // the file descriptor is shared between multiple file handles
    // and while we serialize the requests to FileData, it seems
    // like a good property of this function to avoid changing that
    // state.
    auto len = folly::preadNoInt(state->file.fd(), buf, sizeof(buf), off);
    if (len == 0) {
      break;
    }
    if (len == -1) {
      folly::throwSystemError();
    }
    SHA1_Update(&ctx, buf, len);
    off += len;
  }

  uint8_t digest[SHA_DIGEST_LENGTH];
  SHA1_Final(digest, &ctx);
  auto sha1 = Hash(folly::ByteRange(digest, sizeof(digest)));
  storeSha1(state, sha1);
  return sha1;
}

void FileInode::storeSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state,
    Hash sha1) {
  try {
    fsetxattr(state->file.fd(), kXattrSha1, sha1.toString());
    state->sha1Valid = true;
  } catch (const std::exception& ex) {
    // If something goes wrong storing the attribute just log a warning
    // and leave sha1Valid as false.  We'll have to recompute the value
    // next time we need it.
    XLOG(WARNING) << "error setting SHA1 attribute in the overlay: "
                  << folly::exceptionStr(ex);
  }
}

// Gets the in-memory timestamps of the inode.
InodeBase::InodeTimestamps FileInode::getTimestamps() const {
  auto state = state_.rlock();
  return state->timeStamps;
}

folly::Future<folly::Unit> FileInode::prefetch() {
  // Careful to only hold the lock while fetching a copy of the hash.
  return folly::via(getMount()->getThreadPool().get()).then([this] {
    if (auto hash = state_.rlock()->hash) {
      getObjectStore()->getBlobMetadata(*hash);
    }
  });
}

void FileInode::updateOverlayHeader() const {
  auto state = state_.wlock();
  if (state->file) {
    // File is a materialized file
    Overlay::updateTimestampToHeader(state->file.fd(), state->timeStamps);
  }
}
} // namespace eden
} // namespace facebook
