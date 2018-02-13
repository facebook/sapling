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
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/UnboundedQueueThreadPool.h"
#include "eden/fs/utils/XAttr.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
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
  if (!hash.hasValue()) {
    // File is materialized; read out the timestamps but don't keep it open.
    auto filePath = inode->getLocalPath();
    (void)Overlay::openFile(
        filePath.c_str(), Overlay::kHeaderIdentifierFile, timeStamps);
    tag = MATERIALIZED_IN_OVERLAY;
  } else {
    timeStamps.setAll(lastCheckoutTime);
    tag = NOT_LOADED;
  }

  checkInvariants();
}

FileInode::State::State(
    FileInode* inode,
    mode_t m,
    const timespec& creationTime)
    : tag(MATERIALIZED_IN_OVERLAY), mode(m) {
  timeStamps.setAll(creationTime);
  checkInvariants();
}

void FileInode::State::State::checkInvariants() {
  switch (tag) {
    case NOT_LOADED:
      CHECK(hash);
      CHECK(!blobLoadingPromise);
      CHECK(!blob);
      CHECK(!file);
      CHECK(!sha1Valid);
      return;
    case BLOB_LOADING:
      CHECK(hash);
      CHECK(blobLoadingPromise);
      CHECK(!blob);
      CHECK(!file);
      CHECK(!sha1Valid);
      return;
    case BLOB_LOADED:
      CHECK(hash);
      CHECK(!blobLoadingPromise);
      CHECK(blob);
      CHECK(!file);
      CHECK(!sha1Valid);
      DCHECK_EQ(blob->getHash(), hash.value());
      return;
    case MATERIALIZED_IN_OVERLAY:
      // 'materialized'
      CHECK(!hash);
      CHECK(!blobLoadingPromise);
      CHECK(!blob);
      if (file) {
        CHECK_GT(openCount, 0);
      }
      if (openCount == 0) {
        // file is lazily set, so the only interesting assertion is
        // that it's not open if openCount is zero.
        CHECK(!file);
      }
      return;
  }

  XLOG(FATAL) << "Unexpected tag value: " << tag;
}

void FileInode::State::closeFile() {
  file.close();
}

folly::File FileInode::getFile(FileInode::State& state) const {
  DCHECK(state.isMaterialized())
      << "must only be called for materialized files";

  if (state.openCount > 0 && !state.isFileOpen()) {
    // When opening a file handle to the file, the openCount is incremented but
    // the overlay file is not actually opened.  Instead, it's opened lazily
    // here.
    state.file = folly::File(getLocalPath().c_str(), O_RDWR);
  }

  if (state.isFileOpen()) {
    // Return a non-owning copy of the file object that we already have
    return folly::File(state.file.fd(), /*ownsFd=*/false);
  }

  // We don't have and shouldn't keep a file around, so we return
  // a File temporary instead.
  return folly::File(getLocalPath().c_str(), O_RDWR);
}

/*
 * Defined State Destructor explicitly to avoid including
 * some header files in FileInode.h
 */
FileInode::State::~State() = default;

std::tuple<FileInodePtr, FileInode::FileHandlePtr> FileInode::create(
    fusell::InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    folly::File&& file,
    timespec ctime) {
  // The FileInode is in MATERIALIZED_IN_OVERLAY state.
  auto inode = FileInodePtr::makeNew(
      ino, parentInode, name, mode, std::move(file), ctime);

  return inode->state_.withWLock([&](auto& state) {
    auto fileHandle = std::make_shared<FileHandle>(
        inode, [&state] { fileHandleDidOpen(state); });
    state.file = std::move(file);
    DCHECK_EQ(state.openCount, 1)
        << "open count cannot be anything other than 1";
    return std::make_tuple(inode, fileHandle);
  });
}

// The FileInode is in NOT_LOADED or MATERIALIZED_IN_OVERLAY state.
FileInode::FileInode(
    fusell::InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    const folly::Optional<Hash>& hash)
    : InodeBase(ino, mode_to_dtype(mode), std::move(parentInode), name),
      state_(
          folly::in_place,
          this,
          mode,
          hash,
          getMount()->getLastCheckoutTime()) {}

// The FileInode is in MATERIALIZED_IN_OVERLAY state.
FileInode::FileInode(
    fusell::InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    folly::File&& file,
    timespec ctime)
    : InodeBase(ino, mode_to_dtype(mode), std::move(parentInode), name),
      state_(folly::in_place, this, mode, ctime) {}

folly::Future<fusell::Dispatcher::Attr> FileInode::getattr() {
  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  return stat().then(
      [](const struct stat& st) { return fusell::Dispatcher::Attr{st}; });
}

folly::Future<fusell::Dispatcher::Attr> FileInode::setInodeAttr(
    const fuse_setattr_in& attr) {
  // Minor optimization: if we know that the file is being completely truncated
  // as part of this operation, there's no need to fetch the underlying data,
  // so pass on the truncate flag our underlying open call

  bool truncate = (attr.valid & FATTR_SIZE) && attr.size == 0;
  auto future = truncate ? (materializeAndTruncate(), makeFuture())
                         : materializeForWrite();
  return future.then([self = inodePtrFromThis(), attr]() {
    self->materializeInParent();

    auto result = fusell::Dispatcher::Attr{self->getMount()->initStatData()};

    auto state = self->state_.wlock();
    CHECK_EQ(State::MATERIALIZED_IN_OVERLAY, state->tag)
        << "Must have a file in the overlay at this point";
    auto file = self->getFile(*state);

    // Set the size of the file when FATTR_SIZE is set
    if (attr.valid & FATTR_SIZE) {
      checkUnixError(ftruncate(file.fd(), attr.size + Overlay::kHeaderLength));
    }

    if (attr.valid & FATTR_MODE) {
      // The mode data is stored only in inode_->state_.
      // (We don't set mode bits on the overlay file as that may incorrectly
      // prevent us from reading or writing the overlay data).
      // Make sure we preserve the file type bits, and only update
      // permissions.
      state->mode = (state->mode & S_IFMT) | (07777 & attr.mode);
    }

    // Set in-memory timeStamps
    state->timeStamps.setattrTimes(self->getClock(), attr);

    // We need to call fstat function here to get the size of the overlay
    // file. We might update size in the result while truncating the file
    // when FATTR_SIZE flag is set but when the flag is not set we
    // have to return the correct size of the file even if some size is sent
    // in attr.st.st_size.
    struct stat overlayStat;
    checkUnixError(fstat(file.fd(), &overlayStat));
    result.st.st_ino = self->getNodeId();
    result.st.st_size = overlayStat.st_size - Overlay::kHeaderLength;
    result.st.st_atim = state->timeStamps.atime;
    result.st.st_ctim = state->timeStamps.ctime;
    result.st.st_mtim = state->timeStamps.mtime;
    result.st.st_mode = state->mode;
    result.st.st_nlink = 1;
    updateBlockCount(result.st);

    state->checkInvariants();

    // Update the Journal
    self->updateJournal();
    return result;
  });
}

folly::Future<std::string> FileInode::readlink() {
  if (dtype_t::Symlink != getType()) {
    // man 2 readlink says:  EINVAL The named file is not a symbolic link.
    throw InodeError(EINVAL, inodePtrFromThis(), "not a symlink");
  }

  // The symlink contents are simply the file contents!
  return readAll();
}

void FileInode::fileHandleDidOpen(State& state) {
  // Don't immediately open the file when transitioning from 0 to 1. Open it
  // when getFile() is called.
  state.openCount += 1;
}

void FileInode::fileHandleDidClose() {
  auto state = state_.wlock();
  DCHECK_GT(state->openCount, 0);
  if (--state->openCount == 0) {
    switch (state->tag) {
      case State::BLOB_LOADED:
        state->blob.reset();
        state->tag = State::NOT_LOADED;
        break;
      case State::MATERIALIZED_IN_OVERLAY:
        // TODO: Before closing the file handle, it might make sense to write
        // in-memory timestamps into the overlay, even if the inode remains in
        // memory. This would ensure timestamps persist even if the edenfs
        // process crashes or otherwise exits without unloading all inodes.
        state->closeFile();
        break;
      default:
        break;
    }
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

folly::Future<std::shared_ptr<fusell::FileHandle>> FileInode::open(int flags) {
  if (dtype_t::Symlink == getType()) {
    // Linux reports ELOOP if you try to open a symlink with O_NOFOLLOW set.
    // Since it isn't clear whether FUSE will allow this to happen, this
    // is a speculative defense against that happening; the O_PATH flag
    // does allow a file handle to be opened on a symlink on Linux,
    // but does not allow it to be used for real IO operations.  We're
    // punting on handling those situations here for now.
    throw InodeError(ELOOP, inodePtrFromThis(), "is a symlink");
  }

  auto fileHandle = state_.withWLock([&](auto& state) {
    // Creating the FileHandle increments openCount, which causes the truncation
    // and materialization paths to cache the overlay's file handle in the
    // state.
    return std::make_shared<FileHandle>(
        inodePtrFromThis(), [&state] { fileHandleDidOpen(state); });
  });

  if (flags & O_TRUNC) {
    materializeAndTruncate();
  } else if (flags & (O_RDWR | O_WRONLY | O_CREAT)) {
    // Begin materializing the data into the overlay, but return the FileHandle
    // immediately.
    (void)materializeForWrite();
  } else {
    // Begin prefetching the data as it's likely to be needed soon.
    (void)ensureDataLoaded();
  }

  return fileHandle;
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
  if (dtype_t::Regular == getType()) {
    attributes.emplace_back(kXattrSha1.str());
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

Future<Hash> FileInode::getSha1() {
  auto state = state_.wlock();
  state->checkInvariants();

  switch (state->tag) {
    case State::NOT_LOADED:
    case State::BLOB_LOADING:
    case State::BLOB_LOADED:
      // If a file is not materialized it should have a hash value.
      return getObjectStore()
          ->getBlobMetadata(state->hash.value())
          .then([](const BlobMetadata& metadata) { return metadata.sha1; });
    case State::MATERIALIZED_IN_OVERLAY:
      auto file = getFile(*state);
      if (state->sha1Valid) {
        auto shaStr = fgetxattr(file.fd(), kXattrSha1);
        if (!shaStr.empty()) {
          return Hash(shaStr);
        }
      }
      return recomputeAndStoreSha1(state, file);
  }

  XLOG(FATAL) << "FileInode in illegal state: " << state->tag;
}

folly::Future<struct stat> FileInode::stat() {
  return ensureDataLoaded().then([self = inodePtrFromThis()]() {
    auto st = self->getMount()->initStatData();
    st.st_nlink = 1;
    st.st_ino = self->getNodeId();

    auto state = self->state_.wlock();

    if (state->tag == State::MATERIALIZED_IN_OVERLAY) {
      auto file = self->getFile(*state);
      // We are calling fstat only to get the size of the file.
      struct stat overlayStat;
      checkUnixError(fstat(file.fd(), &overlayStat));

      if (overlayStat.st_size < Overlay::kHeaderLength) {
        auto filePath = self->getLocalPath();
        EDEN_BUG() << "Overlay file " << filePath
                   << " is too short for header: size=" << overlayStat.st_size;
      }
      st.st_size = overlayStat.st_size - Overlay::kHeaderLength;
    } else {
      // blob is guaranteed set because ensureDataLoaded() returns a FileHandle
      // so openCount > 0.
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
    updateBlockCount(st);

    return st;
  });
}

void FileInode::updateBlockCount(struct stat& st) {
  // Compute a value to store in st_blocks based on st_size.
  // Note that st_blocks always refers to 512 byte blocks, regardless of the
  // value we report in st.st_blksize.
  static constexpr off_t kBlockSize = 512;
  st.st_blocks = ((st.st_size + kBlockSize - 1) / kBlockSize);
}

void FileInode::flush(uint64_t /* lock_owner */) {
  // This is called by FUSE when a file handle is closed.
  // https://github.com/libfuse/libfuse/wiki/FAQ#which-method-is-called-on-the-close-system-call
  // We have no write buffers, so there is nothing for us to flush,
  // but let's take this opportunity to update the sha1 attribute.
  auto state = state_.wlock();
  if (state->isFileOpen() && !state->sha1Valid) {
    recomputeAndStoreSha1(state, state->file);
  }
  state->checkInvariants();
}

void FileInode::fsync(bool datasync) {
  auto state = state_.wlock();
  if (!state->isFileOpen()) {
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
  // TODO: A program that issues a series of write() and fsync() syscalls (for
  // example, when logging to a file), would exhibit quadratic behavior here.
  // This should either not recompute SHA-1 here or instead remember if the
  // prior SHA-1 was actually used.
  if (!state->sha1Valid) {
    recomputeAndStoreSha1(state, state->file);
  }
}

folly::Future<std::string> FileInode::readAll() {
  return ensureDataLoaded().then([self = inodePtrFromThis()] {
    // We need to take the wlock instead of the rlock because the lseek() call
    // modifies the file offset of the file descriptor.
    auto state = self->state_.wlock();
    std::string result;
    switch (state->tag) {
      case State::MATERIALIZED_IN_OVERLAY: {
        auto file = self->getFile(*state);
        auto rc = lseek(file.fd(), Overlay::kHeaderLength, SEEK_SET);
        folly::checkUnixError(rc, "unable to seek in materialized FileInode");
        folly::readFile(file.fd(), result);
        break;
      }
      case State::BLOB_LOADED: {
        const auto& contentsBuf = state->blob->getContents();
        folly::io::Cursor cursor(&contentsBuf);
        result = cursor.readFixedString(contentsBuf.computeChainDataLength());
        break;
      }
      default:
        EDEN_BUG()
            << "neither materialized nor loaded after ensureDataLoaded()";
    }

    // We want to update atime after the read operation.
    state->timeStamps.atime = self->getNow();
    return result;
  });
}

fusell::BufVec FileInode::read(size_t size, off_t off) {
  // It's potentially possible here to optimize a fast path here only requiring
  // a read lock.  However, since a write lock is required to update atime and
  // cache the file handle in the case of a materialized file, do the simple
  // thing and just acquire a write lock.

  auto state = state_.wlock();
  state->checkInvariants();
  SCOPE_SUCCESS {
    state->timeStamps.atime = getNow();
  };

  if (state->tag == State::MATERIALIZED_IN_OVERLAY) {
    auto file = getFile(*state);
    auto buf = folly::IOBuf::createCombined(size);
    auto res = ::pread(
        file.fd(), buf->writableBuffer(), size, off + Overlay::kHeaderLength);

    checkUnixError(res);
    buf->append(res);
    return fusell::BufVec{std::move(buf)};
  } else {
    // read() is either called by the FileHandle or FileInode.  They must
    // guarantee openCount > 0.
    CHECK(state->blob);
    auto buf = state->blob->getContents();
    folly::io::Cursor cursor(&buf);

    if (!cursor.canAdvance(off)) {
      // Seek beyond EOF.  Return an empty result.
      return fusell::BufVec{folly::IOBuf::wrapBuffer("", 0)};
    }

    cursor.skip(off);

    std::unique_ptr<folly::IOBuf> result;
    cursor.cloneAtMost(result, size);

    return fusell::BufVec{std::move(result)};
  }
}

folly::Future<size_t> FileInode::write(fusell::BufVec&& buf, off_t off) {
  auto state = state_.wlock();

  if (State::MATERIALIZED_IN_OVERLAY != state->tag) {
    // Not open for write, so wait until it is.
    return materializeForWrite().then(
        [self = inodePtrFromThis(), buf = buf.copyData(), off]() mutable {
          return self->write(StringPiece{buf}, off);
        });
  }

  auto file = getFile(*state);

  state->sha1Valid = false;
  auto vec = buf.getIov();
  auto xfer = ::pwritev(
      file.fd(), vec.data(), vec.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);

  // Update mtime and ctime on write systemcall.
  const auto now = getNow();
  state->timeStamps.mtime = now;
  state->timeStamps.ctime = now;

  return xfer;
}

folly::Future<size_t> FileInode::write(folly::StringPiece data, off_t off) {
  auto state = state_.wlock();

  if (State::MATERIALIZED_IN_OVERLAY != state->tag) {
    // Not open for write, so wait until it is.
    return materializeForWrite().then(
        [self = inodePtrFromThis(), data = data.str(), off]() mutable {
          return self->write(StringPiece{data}, off);
        });
  }
  auto file = getFile(*state);

  state->sha1Valid = false;
  auto xfer = ::pwrite(
      file.fd(), data.data(), data.size(), off + Overlay::kHeaderLength);
  checkUnixError(xfer);

  // Update mtime and ctime on write systemcall.
  const auto now = getNow();
  state->timeStamps.mtime = now;
  state->timeStamps.ctime = now;

  return xfer;
}

// Waits until inode is either in 'loaded' or 'materialized' state.
Future<FileInode::FileHandlePtr> FileInode::ensureDataLoaded() {
  folly::Optional<Future<FileHandlePtr>> resultFuture;
  auto blobFuture = Future<std::shared_ptr<const Blob>>::makeEmpty();

  {
    // Scope the lock so that we can't deadlock on the completion of
    // the blobFuture below.
    auto state = state_.wlock();

    state->checkInvariants();
    SCOPE_SUCCESS {
      state->checkInvariants();
    };

    switch (state->tag) {
      case State::BLOB_LOADING:
        // If we're already loading, latch on to the in-progress load
        return state->blobLoadingPromise->getFuture();

      case State::BLOB_LOADED:
      case State::MATERIALIZED_IN_OVERLAY:
        // Nothing to do if loaded or materialized.
        return makeFuture(std::make_shared<FileHandle>(
            inodePtrFromThis(), [&state] { fileHandleDidOpen(*state); }));

      case State::NOT_LOADED:
        // Start the blob load first in case this throws an exception.
        // Ideally the state transition is no-except in tandem with the
        // Future's .then call.
        blobFuture = getObjectStore()->getBlob(state->hash.value());

        // We need to load the blob data.  Arrange to do so in a way that
        // multiple callers can wait for.
        folly::SharedPromise<FileHandlePtr> promise;
        // The resultFuture will complete after we have loaded the blob
        // and updated state_.
        resultFuture = promise.getFuture();

        // Everything from here through blobFuture.then should be noexcept.
        state->blobLoadingPromise.emplace(std::move(promise));
        state->tag = State::BLOB_LOADING;
        break;
    }
  }

  // Execution only gets here in the NOT_LOADED case, in which case resultFuture
  // is initialized.
  CHECK(resultFuture);

  auto self = inodePtrFromThis(); // separate line for formatting
  blobFuture
      .then([self](folly::Try<std::shared_ptr<const Blob>> tryBlob) {
        auto state = self->state_.wlock();
        state->checkInvariants();

        switch (state->tag) {
          // Since the load doesn't hold the state lock for its duration,
          // sanity check that the inode is still in loading state.
          //
          // Note that FileInode can transition from loading to materialized
          // with a concurrent materializeForWrite(O_TRUNC), in which case the
          // state would have transitioned to 'materialized' before this
          // callback runs.
          case State::BLOB_LOADING: {
            auto promise = std::move(*state->blobLoadingPromise);
            state->blobLoadingPromise.clear();

            if (tryBlob.hasValue()) {
              // Transition to 'loaded' state.
              state->blob = std::move(tryBlob.value());
              state->tag = State::BLOB_LOADED;
              state->checkInvariants();
              // The FileHandle must be allocated while the lock is held so the
              // blob field is set and the openCount incremented atomically, so
              // that no other thread can cause the blob to get unset before
              // openCount is incremented.
              auto result = std::make_shared<FileHandle>(
                  self, [&state] { fileHandleDidOpen(*state); });
              // Call the Future's subscribers while the state_ lock is not
              // held. Even if the FileInode has transitioned to a materialized
              // state, any pending loads must be unblocked.
              state.unlock();
              promise.setValue(std::move(result));
            } else {
              state->tag = State::NOT_LOADED;
              state->checkInvariants();
              // Call the Future's subscribers while the state_ lock is not
              // held. Even if the FileInode has transitioned to a materialized
              // state, any pending loads must be unblocked.
              state.unlock();
              promise.setException(tryBlob.exception());
            }
            break;
          }

          case State::MATERIALIZED_IN_OVERLAY:
            // The load raced with a materializeForWrite(O_TRUNC).  Nothing left
            // to do here: ensureDataLoaded() guarantees `blob` or `file` is
            // defined after its completion, and the
            // materializeForWrite(O_TRUNC) fulfilled the promise.
            break;

          default:
            EDEN_BUG()
                << "Inode left in unexpected state after getBlob() completed";
        }
      })
      .onError([](const std::exception&) {
        // We get here if EDEN_BUG() didn't terminate the process, or if we
        // threw in the preceding block.  Both are bad because we won't
        // automatically propagate the exception to resultFuture and we
        // can't trust the state of anything if we get here.
        // Rather than leaving something hanging, we suicide.
        // We could probably do a bit better with the error handling here :-/
        XLOG(FATAL)
            << "Failed to propagate failure in getBlob(), no choice but to die";
      });

  return std::move(*resultFuture);
}

namespace {
folly::IOBuf createOverlayHeaderFromTimestamps(
    const InodeTimestamps& timestamps) {
  return Overlay::createHeader(
      Overlay::kHeaderIdentifierFile, Overlay::kHeaderVersion, timestamps);
}
} // namespace

Future<Unit> FileInode::materializeForWrite() {
  // Not O_TRUNC, so ensure we have a blob (or are already materialized).
  return ensureDataLoaded().then([self = inodePtrFromThis()]() {
    // Notifying the parent of materialization must happen outside of the lock.
    {
      auto state = self->state_.wlock();

      state->checkInvariants();
      SCOPE_SUCCESS {
        state->checkInvariants();
      };

      if (state->tag == State::MATERIALIZED_IN_OVERLAY) {
        // This conditional will be hit if materializeForWrite is called, issues
        // a load, and then materializeAndTruncate is called before
        // ensureDataLoaded() completes.  The prior O_TRUNC would have completed
        // synchronously and switched the inode into the 'materialized' state,
        // in which case there is nothing left to do here.
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
        self->storeSha1(state, file, metadata.value().sha1);
      } else {
        // Leave the SHA-1 attribute dirty - it is not very likely that a file
        // will be opened for writing, closed without changing, and then have
        // its SHA-1 queried via Thrift or xattr. If so, the SHA-1 will be
        // recomputed as needed. That said, it's perhaps cheaper to hash now
        // (SHA-1 is hundreds of MB/s) while the data is accessible in the blob
        // than to read the file out of the overlay later.
      }

      // ensureDataLoaded() returns a FileHandle; therefore openCount must be
      // positive; therefore it's okay to set file.
      CHECK_GT(state->openCount, 0);

      // Update the FileInode to indicate that we are materialized now.
      state->blob.reset();
      state->hash = folly::none;
      state->file = std::move(file);
      state->tag = State::MATERIALIZED_IN_OVERLAY;
    }
    self->materializeInParent();
  });
}

void FileInode::materializeAndTruncate() {
  // Set if in 'loading' state.  Fulfilled outside of the scopes of any locks.
  folly::SharedPromise<FileHandlePtr> sharedPromise;

  // Notifying the parent of materialization must occur outside of the lock.
  bool didMaterialize = false;

  auto try_ = folly::makeTryWith([&] {
    auto state = state_.wlock();
    state->checkInvariants();
    SCOPE_SUCCESS {
      state->checkInvariants();
    };

    folly::File file;
    if (state->isMaterialized()) { // Materialized already.
      file = getFile(*state);
      state->sha1Valid = false;
      checkUnixError(ftruncate(file.fd(), Overlay::kHeaderLength));
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
      file = Overlay::openFile(
          filePath.stringPiece(), Overlay::kHeaderIdentifierFile, timeStamps);

      // Everything below here in the scope should be noexcept to ensure that
      // the state is never partially transitioned.

      // Transition to `loaded`.
      if (state->blobLoadingPromise) { // Loading.
        // Move the promise out so it's fulfilled outside of the lock.
        sharedPromise = std::move(*state->blobLoadingPromise);
        state->blobLoadingPromise.reset();
      } else if (state->blob) { // Loaded.
        state->blob.reset();
      } else { // Not loaded.
      }

      state->hash.reset();
      // If a FileHandle is already open cache the newly-opened file.
      if (state->openCount) {
        state->file = std::move(file);
      }
      state->sha1Valid = false;
      state->tag = State::MATERIALIZED_IN_OVERLAY;
      didMaterialize = true;
    }
    storeSha1(state, file, Hash::sha1(ByteRange{}));

    return std::make_shared<FileHandle>(
        inodePtrFromThis(), [&state] { fileHandleDidOpen(*state); });
  });

  if (didMaterialize) {
    materializeInParent();
  }

  // Fulfill outside of the lock.
  sharedPromise.setTry(std::move(try_));
}

ObjectStore* FileInode::getObjectStore() const {
  return getMount()->getObjectStore();
}

Hash FileInode::recomputeAndStoreSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state,
    const folly::File& file) {
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
    auto len = folly::preadNoInt(file.fd(), buf, sizeof(buf), off);
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
  storeSha1(state, file, sha1);
  return sha1;
}

void FileInode::storeSha1(
    const folly::Synchronized<FileInode::State>::LockedPtr& state,
    const folly::File& file,
    Hash sha1) {
  try {
    fsetxattr(file.fd(), kXattrSha1, sha1.toString());
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
InodeTimestamps FileInode::getTimestamps() const {
  return state_.rlock()->timeStamps;
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
  if (state->isMaterialized()) {
    int fd;
    folly::File temporaryHandle;
    if (state->isFileOpen()) {
      fd = state->file.fd();
    } else {
      // We don't have and shouldn't keep a file around, so we return
      // a temporary file instead.
      temporaryHandle = folly::File(getLocalPath().c_str(), O_RDWR);
      fd = temporaryHandle.fd();
    }

    Overlay::updateTimestampToHeader(fd, state->timeStamps);
  }
}
} // namespace eden
} // namespace facebook
