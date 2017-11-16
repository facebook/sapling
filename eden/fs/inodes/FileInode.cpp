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
    CHECK(!blobLoadingPromise);
    CHECK(!file);
    CHECK(!sha1Valid);
    DCHECK_EQ(blob->getHash(), hash.value());
  } else if (blobLoadingPromise) {
    // 'loading'
    CHECK(hash);
    CHECK(!blob);
    CHECK(!file);
    CHECK(!sha1Valid);
  } else if (file) {
    // 'materialized'
    CHECK(!hash);
    CHECK(!blobLoadingPromise);
    CHECK(!blob);
    CHECK(file);
  } else {
    // 'not loaded'
    CHECK(hash);
    CHECK(!blobLoadingPromise);
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

std::tuple<FileInodePtr, std::shared_ptr<FileHandle>> FileInode::create(
    fuse_ino_t ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    folly::File&& file,
    dev_t rdev) {
  auto inode = FileInodePtr::makeNew(
      ino, parentInode, name, mode, std::move(file), rdev);

  SCOPE_EXIT {
    inode->fileHandleDidClose();
  };
  return std::make_tuple(inode, std::make_shared<FileHandle>(inode));
}

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

// Waits until inode is either in 'loaded' or 'materialized' state.
Future<Unit> FileInode::ensureDataLoaded() {
  Future<Unit> resultFuture;
  auto blobFuture = Future<std::shared_ptr<const Blob>>::makeEmpty();

  {
    // Scope the lock so that we can't deadlock on the completion of
    // the blobFuture below.
    auto state = state_.wlock();

    SCOPE_SUCCESS {
      state->checkInvariants();
    };

    if (state->blobLoadingPromise.hasValue()) {
      // If we're already loading, latch on to the in-progress load
      return state->blobLoadingPromise->getFuture();
    }

    // Nothing to do if loaded or materialized.
    if (state->file || state->blob) {
      return makeFuture();
    }

    // We need to load the blob data.  Arrange to do so in a way that
    // multiple callers can wait for.
    folly::SharedPromise<Unit> promise;
    // The resultFuture will complete after we have loaded the blob
    // and updated state_.
    resultFuture = promise.getFuture();
    // Move the promise into the Optional type that we can test
    // in a subsequent call to ensureDataLoaded().
    state->blobLoadingPromise.emplace(std::move(promise));
    // Start the load of the blob.
    blobFuture = getObjectStore()->getBlob(state->hash.value());
  }

  auto self = inodePtrFromThis(); // separate line for formatting
  blobFuture.then([self](std::shared_ptr<const Blob> blob) {
    folly::SharedPromise<Unit> promise;

    {
      auto state = self->state_.wlock();
      state->checkInvariants();
      SCOPE_SUCCESS {
        state->checkInvariants();
      };

      // Since the load doesn't hold the state lock for its duration,
      // sanity check that the inode is still in loading state.
      //
      // Note that FileInode can transition from loading to materialized
      // with a concurrent materializeForWrite(O_TRUNC), in which case the
      // state would have transitioned to 'materialized' before this
      // callback runs.
      if (state->blobLoadingPromise) {
        // Transition to 'loaded' state.
        state->blob = std::move(blob);
        promise = std::move(*state->blobLoadingPromise);
        state->blobLoadingPromise.clear();
      } else {
        CHECK(state->file);
        // The load raced with a materializeForWrite(O_TRUNC).  Nothing left to
        // do here: ensureDataLoaded() guarantees `blob` or `file` is defined
        // after its completion, and the materializeForWrite(O_TRUNC) fulfilled
        // the promise.
      }
    }

    // Call the Future's subscribers while the state_ lock is not held.
    // Even if the FileInode has transitioned to a materialized state, any
    // pending loads must be unblocked.
    promise.setValue();
  });

  return resultFuture;
}

namespace {
folly::IOBuf createOverlayHeaderFromTimestamps(
    const InodeBase::InodeTimestamps& timestamps) {
  return Overlay::createHeader(
      Overlay::kHeaderIdentifierFile,
      Overlay::kHeaderVersion,
      timestamps.atime,
      timestamps.ctime,
      timestamps.mtime);
}
} // namespace

// Waits until inode is in 'materialized' state.
Future<Unit> FileInode::materializeForWrite(int openFlags) {
  // Fast-path O_TRUNC for all states: blobs never need to be loaded from the
  // backing store.
  if (openFlags & O_TRUNC) {
    materializeAndTruncate();
    return makeFuture();
  }

  // Not O_TRUNC, so ensure we have a blob (or are already materialized).
  return ensureDataLoaded().then([self = inodePtrFromThis()]() {
    auto state = self->state_.wlock();

    state->checkInvariants();
    SCOPE_SUCCESS {
      state->checkInvariants();
    };

    if (state->file) {
      // This conditional will be hit if materializeForWrite is called without
      // O_TRUNC, issues a load, and then materializeForWrite is called _with_
      // O_TRUNC before ensureDataLoaded() completes.  The prior O_TRUNC would
      // have completed synchronously and switched the inode into the
      // 'materialized' state, in which case there is nothing left to do here.
      return;
    }

    // Add header to the overlay File.
    auto header = createOverlayHeaderFromTimestamps(state->timeStamps);
    auto iov = header.getIov();

    auto filePath = self->getLocalPath();

    // state->blob is guaranteed non-null because:
    //   If state->file was set, we would have early exited above.
    //   If not O_TRUNC, then we called ensureDataLoaded().
    CHECK_NOTNULL(state->blob.get());

    // Write the blob contents out to the overlay
    auto contents = state->blob->getContents().getIov();
    iov.insert(iov.end(), contents.begin(), contents.end());

    folly::writeFileAtomic(
        filePath.stringPiece(), iov.data(), iov.size(), 0600);
    InodeTimestamps timeStamps;

    auto file = Overlay::openFile(
        filePath.stringPiece(), Overlay::kHeaderIdentifierFile, timeStamps);
    state->sha1Valid = false;

    // If we have a SHA-1 from the metadata, apply it to the new file.  This
    // saves us from recomputing it again in the case that something opens the
    // file read/write and closes it without changing it.
    auto metadata =
        self->getObjectStore()->getBlobMetadata(state->hash.value());
    if (metadata.isReady()) {
      self->storeSha1(state, metadata.value().sha1);
    } else {
      // Leave the SHA-1 attribute dirty - it is not very likely that a file
      // will be opened for writing, closed without changing, and then have its
      // SHA-1 queried via Thrift or xattr. If so, the SHA-1 will be recomputed
      // as needed. That said, it's perhaps cheaper to hash now (SHA-1 is
      // hundreds of MB/s) while the data is accessible in the blob than to read
      // the file out of the overlay later.
    }

    // Update the FileInode to indicate that we are materialized now
    state->file = std::move(file);
    state->blob.reset();
    state->hash = folly::none;
  });
}

void FileInode::materializeAndTruncate() {
  // Set if in 'loading' state.  Fulfilled outside of the scopes of any locks.
  folly::Optional<folly::SharedPromise<folly::Unit>> sharedPromise;

  auto exceptionWrapper = folly::try_and_catch<const std::exception>([&] {
    auto state = state_.wlock();
    state->checkInvariants();
    SCOPE_SUCCESS {
      state->checkInvariants();
    };

    if (state->file) { // Materialized already.
      state->sha1Valid = false;
      checkUnixError(ftruncate(state->file.fd(), Overlay::kHeaderLength));
      // The timestamps in the overlay header will get updated when the inode is
      // unloaded.
    } else {
      // Add header to the overlay File.
      auto header = createOverlayHeaderFromTimestamps(state->timeStamps);
      auto iov = header.getIov();

      auto filePath = getLocalPath();

      folly::writeFileAtomic(filePath.stringPiece(), iov.data(), iov.size());
      // We don't want to set the in-memory timestamps to the timestamps
      // returned by the below openFile function as we just wrote these
      // timestamps in to overlay using writeFileAtomic.
      InodeTimestamps timeStamps;
      auto file = Overlay::openFile(
          filePath.stringPiece(), Overlay::kHeaderIdentifierFile, timeStamps);

      // Everything below here in the scope should be noexcept to ensure that
      // the state is never partially transitioned.

      // Transition to `loaded`.
      if (state->blobLoadingPromise) { // Loading.
        // Move the promise out so it's fulfilled outside of the lock.
        sharedPromise.emplace(std::move(*state->blobLoadingPromise));
        state->blobLoadingPromise.reset();
      } else if (state->blob) { // Loaded.
        state->blob.reset();
      } else { // Not loaded.
      }

      state->hash.reset();
      state->file = std::move(file);
      state->sha1Valid = false;
    }
    storeSha1(state, Hash::sha1(ByteRange{}));
  });

  // Fulfill outside of the lock.
  if (sharedPromise) {
    if (exceptionWrapper) {
      sharedPromise->setException(exceptionWrapper);
    } else {
      sharedPromise->setValue();
    }
  }
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
