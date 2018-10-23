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
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <openssl/sha.h>
#include "eden/fs/inodes/EdenFileHandle.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"
#include "eden/fs/utils/XAttr.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Synchronized;
using folly::Unit;
using std::shared_ptr;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

/*********************************************************************
 * FileInode::LockedState
 ********************************************************************/

/**
 * LockedState is a helper class that wraps
 * folly::Synchronized<State>::LockedPtr
 *
 * It implements operator->() and operator*() so it can be used just like
 * LockedPtr.  However, it also is capable of managing a reference count
 * to State::openCount, and decrementing this count when it is destroyed, or
 * transferring this count to a new EdenFileHandle object.
 */
class FileInode::LockedState {
 public:
  explicit LockedState(FileInode* inode) : ptr_{inode->state_.wlock()} {}
  explicit LockedState(const FileInodePtr& inode)
      : ptr_{inode->state_.wlock()} {}

  LockedState(LockedState&&) = default;
  LockedState& operator=(LockedState&&) = default;

  ~LockedState();

  State* operator->() const {
    return ptr_.operator->();
  }
  State& operator*() const {
    return ptr_.operator*();
  }

  bool isNull() const {
    return ptr_.isNull();
  }
  explicit operator bool() const {
    return !ptr_.isNull();
  }

  /**
   * Explicitly unlock the LockedState object before it is destroyed.
   */
  void unlock();

  /**
   * Unlock the state and create a new EdenFileHandle object.
   */
  std::shared_ptr<EdenFileHandle> unlockAndCreateHandle(FileInodePtr inode);

  /**
   * Create an EdenFileHandle object.
   *
   * Beware that you must pass in an EdenFileHandle object that exists in a
   * higher-level scope than the LockedState object itself.  You must ensure
   * that the LockedState object is destroyed before the EdenFileHandle
   * object.
   */
  void createHandleInOuterScope(
      FileInodePtr inode,
      std::shared_ptr<EdenFileHandle>* outHandle);

  /**
   * Ensure that state->file is an open File object.
   *
   * This method may only be called when the state tag is
   * MATERIALIZED_IN_OVERLAY.
   */
  void ensureFileOpen(const FileInode* inode);

  /**
   * This moves the file into the MATERIALIZED_IN_OVERLAY state, setting
   * state->file.
   *
   * This updates state->tag and state->file, and clears state->blob,
   * state-hash, and state->sha1Valid.
   *
   * This also implicitly ensures that this LockedState has an open refcount.
   */
  void setMaterialized(folly::File&& file);

  /**
   * Increment the state's open count.
   *
   * This should generally be called when setting the blob or file object in
   * the state, to ensure that the blob or file is destroyed when the state
   * lock is released if it is not still referenced by an EdenFileHandle
   * object.
   *
   * This reference count will automatically be decremented again when the
   * LockedState is destroyed.  This can only be called at most once on a
   * LockedState object--it is not valid to call incOpenCount() on a
   * LockedState that already has a reference count.
   */
  void incOpenCount();

  bool hasOpenCount() const {
    return hasOpenRefcount_;
  }

 private:
  folly::Synchronized<State>::LockedPtr ptr_;
  bool hasOpenRefcount_{false};
};

FileInode::LockedState::~LockedState() {
  if (!ptr_) {
    return;
  }
  if (hasOpenRefcount_) {
    ptr_->decOpenCount();
  }
  // Check the state invariants every time we release the lock
  ptr_->checkInvariants();
}

void FileInode::LockedState::unlock() {
  if (hasOpenRefcount_) {
    ptr_->decOpenCount();
  }
  ptr_->checkInvariants();
  ptr_.unlock();
}

std::shared_ptr<EdenFileHandle> FileInode::LockedState::unlockAndCreateHandle(
    FileInodePtr inode) {
  std::shared_ptr<EdenFileHandle> handle;
  createHandleInOuterScope(std::move(inode), &handle);
  // Beware: creating the EdenFileHandle should be the very last thing we do
  // before unlocking the state.  If we throw an exception after creating the
  // EdenFileHandle but while still holding the state lock we will deadlock in
  // the EdenFileHandle destructor, which acquires the state lock.
  ptr_.unlock();
  return handle;
}

void FileInode::LockedState::createHandleInOuterScope(
    FileInodePtr inode,
    std::shared_ptr<EdenFileHandle>* outHandle) {
  if (!hasOpenRefcount_) {
    ptr_->incOpenCount();
    hasOpenRefcount_ = true;
  }

  ptr_->checkInvariants();
  *outHandle =
      std::make_shared<EdenFileHandle>(std::move(inode), &hasOpenRefcount_);
}

void FileInode::LockedState::incOpenCount() {
  CHECK(!hasOpenRefcount_);
  ptr_->incOpenCount();
  hasOpenRefcount_ = true;
}

void FileInode::LockedState::ensureFileOpen(const FileInode* inode) {
  DCHECK(ptr_->isMaterialized())
      << "must only be called for materialized files";

  if (!hasOpenRefcount_) {
    ptr_->incOpenCount();
    hasOpenRefcount_ = true;
  }

  if (!ptr_->isFileOpen()) {
    // When opening a file handle to the file, the openCount is incremented but
    // the overlay file is not actually opened.  Instead, it's opened lazily
    // here.
    ptr_->file =
        inode->getMount()->getOverlay()->openFileNoVerify(inode->getNodeId());
  }
}

void FileInode::LockedState::setMaterialized(folly::File&& file) {
  if (!hasOpenRefcount_) {
    ptr_->incOpenCount();
    hasOpenRefcount_ = true;
  }

  ptr_->file = std::move(file);
  ptr_->hash.reset();
  ptr_->blob.reset();
  ptr_->tag = State::MATERIALIZED_IN_OVERLAY;
  ptr_->sha1Valid = false;
}

/*********************************************************************
 * Implementations of FileInode private template methods
 * These definitions need to appear before any functions that use them.
 ********************************************************************/

template <typename Fn>
typename folly::futures::detail::callableResult<FileInode::LockedState, Fn>::
    Return
    FileInode::runWhileDataLoaded(LockedState state, Fn&& fn) {
  auto future = Future<FileHandlePtr>::makeEmpty();
  switch (state->tag) {
    case State::BLOB_LOADED:
      // We can run the function immediately
      return folly::makeFutureWith(
          [&] { return std::forward<Fn>(fn)(std::move(state)); });
    case State::MATERIALIZED_IN_OVERLAY:
      // Open the file, then run the function
      state.ensureFileOpen(this);
      return folly::makeFutureWith(
          [&] { return std::forward<Fn>(fn)(std::move(state)); });
    case State::BLOB_LOADING:
      // If we're already loading, latch on to the in-progress load
      future = state->blobLoadingPromise->getFuture();
      state.unlock();
      break;
    case State::NOT_LOADED:
      future = startLoadingData(std::move(state));
      break;
  }

  return std::move(future).thenValue([self = inodePtrFromThis(),
                                      fn = std::forward<Fn>(fn)](
                                         FileHandlePtr /* handle */) mutable {
    // Simply call runWhileDataLoaded() again when we we finish loading the blob
    // data.  The state should be BLOB_LOADED or MATERIALIZED_IN_OVERLAY this
    // time around.
    auto stateLock = LockedState{self};
    DCHECK(
        stateLock->tag == State::BLOB_LOADED ||
        stateLock->tag == State::MATERIALIZED_IN_OVERLAY)
        << "unexpected FileInode state after loading: " << stateLock->tag;
    return self->runWhileDataLoaded(std::move(stateLock), std::forward<Fn>(fn));
  });
}

template <typename Fn>
typename folly::futures::detail::callableResult<FileInode::LockedState, Fn>::
    Return
    FileInode::runWhileMaterialized(LockedState state, Fn&& fn) {
  auto future = Future<FileHandlePtr>::makeEmpty();
  switch (state->tag) {
    case State::BLOB_LOADED: {
      // We have the blob data loaded.
      // Materialize the file now.
      materializeNow(state);
      // Call materializeInParent before we return, after we are
      // sure the state lock has been released.  This does mean that our parent
      // won't have updated our state until after the caller's function runs,
      // but this is okay.  There is always a brief gap between when we
      // materialize ourself and when our parent gets updated to indicate this.
      // If we do crash during this period it is not too unreasonable that
      // recent change right before the crash might be reverted to their
      // non-materialized state.
      SCOPE_EXIT {
        CHECK(state.isNull());
        materializeInParent();
      };
      // Note that we explicitly create a temporary LockedState object
      // to pass to the caller to ensure that the state lock will be released
      // when they return, even if the caller's function accepts the state as
      // an rvalue-reference and does not release it themselves.
      return folly::makeFutureWith(
          [&] { return std::forward<Fn>(fn)(LockedState{std::move(state)}); });
    }
    case State::MATERIALIZED_IN_OVERLAY:
      // Open the file, then run the function
      state.ensureFileOpen(this);
      return folly::makeFutureWith(
          [&] { return std::forward<Fn>(fn)(LockedState{std::move(state)}); });
    case State::BLOB_LOADING:
      // If we're already loading, latch on to the in-progress load
      future = state->blobLoadingPromise->getFuture();
      state.unlock();
      break;
    case State::NOT_LOADED:
      future = startLoadingData(std::move(state));
      break;
  }

  return std::move(future).thenValue(
      [self = inodePtrFromThis(),
       fn = std::forward<Fn>(fn)](FileHandlePtr /* handle */) mutable {
        // Simply call runWhileDataLoaded() again when we we finish loading the
        // blob data.  The state should be BLOB_LOADED or
        // MATERIALIZED_IN_OVERLAY this time around.
        auto stateLock = LockedState{self};
        DCHECK(
            stateLock->tag == State::BLOB_LOADED ||
            stateLock->tag == State::MATERIALIZED_IN_OVERLAY)
            << "unexpected FileInode state after loading: " << stateLock->tag;
        return self->runWhileMaterialized(
            std::move(stateLock), std::forward<Fn>(fn));
      });
}

template <typename Fn>
typename std::result_of<Fn(FileInode::LockedState&&)>::type
FileInode::truncateAndRun(LockedState state, Fn&& fn) {
  auto future = Future<FileHandlePtr>::makeEmpty();
  switch (state->tag) {
    case State::NOT_LOADED:
    case State::BLOB_LOADED:
    case State::BLOB_LOADING: {
      // We are not materialized yet.  We need to materialize the file now.
      //
      // Note that we have to be pretty careful about ordering of operations
      // here and how we behave if an exception is thrown at any point.  We
      // want to:
      // - Truncate the file.
      // - Invoke the input function with the state lock still held.
      // - Release the state lock
      // - Assuming we successfully materialized the file, mark ourself
      //   materialized in our parent TreeInode.
      // - If we successfully materialized the file and were in the
      //   BLOB_LOADING state, fulfill the blobLoadingPromise.
      std::shared_ptr<EdenFileHandle> handle;
      folly::Optional<folly::SharedPromise<FileHandlePtr>> loadingPromise;
      SCOPE_EXIT {
        if (loadingPromise) {
          loadingPromise->setValue(std::move(handle));
        }
      };

      // If we are currently in the BLOB_LOADING state, we first need to create
      // an EdenFileHandle object to use to fulfill the blobLoadingPromise.
      // We do this early on so that we cannot fail to create the file handle
      // after we have successfully materialized the file.
      //
      // We move the LockedState object into an inner scope when we do this, to
      // ensure that the LockedState always gets destroyed before the
      // EdenFileHandle object.  The EdenFileHandle destructor requires
      // acquiring the state lock itself, so the lock cannot still be held when
      // the EdenFileHandle destructor runs.
      {
        LockedState innerState(std::move(state));
        if (innerState->tag == State::BLOB_LOADING) {
          innerState.createHandleInOuterScope(inodePtrFromThis(), &handle);
        }

        // Call materializeAndTruncate()
        materializeAndTruncate(innerState);

        // Now that materializeAndTruncate() has succeeded, extract the
        // blobLoadingPromise so we can fulfill it as we exit.
        loadingPromise = std::move(innerState->blobLoadingPromise);
        // Also call materializeInParent() as we exit, before fulfilling the
        // blobLoadingPromise.
        SCOPE_EXIT {
          CHECK(innerState.isNull());
          materializeInParent();
        };

        // Now invoke the input function.
        // Note that we explicitly create a temporary LockedState object
        // to pass to the caller to ensure that the state lock will be released
        // when they return, even if the caller's function accepts the state as
        // an rvalue-reference and does not release it themselves.
        return std::forward<Fn>(fn)(LockedState{std::move(innerState)});
      }
    }
    case State::MATERIALIZED_IN_OVERLAY:
      // We are already materialized.
      // Truncate the file in the overlay, then call the function.
      truncateInOverlay(state);
      return std::forward<Fn>(fn)(std::move(state));
  }

  XLOG(FATAL) << "unexpected FileInode state " << state->tag;
}

/*********************************************************************
 * FileInode::State methods
 ********************************************************************/

FileInodeState::FileInodeState(const folly::Optional<Hash>& h) : hash(h) {
  tag = hash ? NOT_LOADED : MATERIALIZED_IN_OVERLAY;

  checkInvariants();
}

FileInodeState::FileInodeState() : tag(MATERIALIZED_IN_OVERLAY) {
  checkInvariants();
}

/*
 * Define FileInodeState destructor explicitly to avoid including
 * some header files in FileInode.h
 */
FileInodeState::~FileInodeState() = default;

void FileInodeState::checkInvariants() {
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

void FileInodeState::incOpenCount() {
  ++openCount;
}

void FileInodeState::decOpenCount() {
  DCHECK_GT(openCount, 0);
  --openCount;
  if (openCount == 0) {
    switch (tag) {
      case BLOB_LOADED:
        blob.reset();
        tag = NOT_LOADED;
        break;
      case MATERIALIZED_IN_OVERLAY:
        // TODO: Before closing the file handle, it might make sense to write
        // in-memory timestamps into the overlay, even if the inode remains in
        // memory. This would ensure timestamps persist even if the edenfs
        // process crashes or otherwise exits without unloading all inodes.
        file.close();
        break;
      default:
        break;
    }
  }
}

/*********************************************************************
 * FileInode methods
 ********************************************************************/

std::tuple<FileInodePtr, FileInode::FileHandlePtr> FileInode::create(
    InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t mode,
    InodeTimestamps initialTimestamps,
    folly::File&& file) {
  // The FileInode is in MATERIALIZED_IN_OVERLAY state.
  auto inode =
      FileInodePtr::makeNew(ino, parentInode, name, mode, initialTimestamps);

  auto state = LockedState{inode};
  state.incOpenCount();
  state->file = std::move(file);
  DCHECK_EQ(state->openCount, 1)
      << "open count cannot be anything other than 1";
  return std::make_tuple(inode, state.unlockAndCreateHandle(inode));
}

// The FileInode is in NOT_LOADED or MATERIALIZED_IN_OVERLAY state.
FileInode::FileInode(
    InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t initialMode,
    folly::Function<folly::Optional<InodeTimestamps>()> initialTimestampsFn,
    const folly::Optional<Hash>& hash)
    : Base(
          ino,
          initialMode,
          std::move(initialTimestampsFn),
          std::move(parentInode),
          name),
      state_(folly::in_place, hash) {}

// The FileInode is in MATERIALIZED_IN_OVERLAY state.
FileInode::FileInode(
    InodeNumber ino,
    TreeInodePtr parentInode,
    PathComponentPiece name,
    mode_t initialMode,
    InodeTimestamps initialTimestamps)
    : Base(ino, initialMode, initialTimestamps, std::move(parentInode), name),
      state_(folly::in_place) {}

folly::Future<Dispatcher::Attr> FileInode::getattr() {
  // Future optimization opportunity: right now, if we have not already
  // materialized the data from the entry, we have to materialize it
  // from the store.  If we augmented our metadata we could avoid this,
  // and this would speed up operations like `ls`.
  return stat().thenValue(
      [](const struct stat& st) { return Dispatcher::Attr{st}; });
}

folly::Future<Dispatcher::Attr> FileInode::setattr(
    const fuse_setattr_in& attr) {
  // If this file is inside of .eden it cannot be reparented, so getParentRacy()
  // is okay.
  auto parent = getParentRacy();
  if (parent && parent->getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return folly::makeFuture<Dispatcher::Attr>(
        InodeError(EPERM, inodePtrFromThis()));
  }

  auto setAttrs = [self = inodePtrFromThis(), attr](LockedState&& state) {
    auto result = Dispatcher::Attr{self->getMount()->initStatData()};

    DCHECK_EQ(State::MATERIALIZED_IN_OVERLAY, state->tag)
        << "Must have a file in the overlay at this point";
    DCHECK(state->isFileOpen());

    // Set the size of the file when FATTR_SIZE is set
    if (attr.valid & FATTR_SIZE) {
      checkUnixError(
          ftruncate(state->file.fd(), attr.size + Overlay::kHeaderLength));
    }

    auto metadata = self->getMount()->getInodeMetadataTable()->modifyOrThrow(
        self->getNodeId(), [&](auto& metadata) {
          metadata.updateFromAttr(self->getClock(), attr);
        });

    // We need to call fstat function here to get the size of the overlay
    // file. We might update size in the result while truncating the file
    // when FATTR_SIZE flag is set but when the flag is not set we
    // have to return the correct size of the file even if some size is sent
    // in attr.st.st_size.
    struct stat overlayStat;
    checkUnixError(fstat(state->file.fd(), &overlayStat));
    result.st.st_ino = self->getNodeId().get();
    result.st.st_size = overlayStat.st_size - Overlay::kHeaderLength;
    metadata.applyToStat(result.st);
    result.st.st_nlink = 1;
    updateBlockCount(result.st);

    // Update the Journal
    self->updateJournal();
    return result;
  };

  // Minor optimization: if we know that the file is being completely truncated
  // as part of this operation, there's no need to fetch the underlying data,
  // so use truncateAndRun() rather than runWhileMaterialized()
  bool truncate = (attr.valid & FATTR_SIZE) && attr.size == 0;
  auto state = LockedState{this};
  if (truncate) {
    return truncateAndRun(std::move(state), setAttrs);
  } else {
    return runWhileMaterialized(std::move(state), setAttrs);
  }
}

folly::Future<std::string> FileInode::readlink() {
  if (dtype_t::Symlink != getType()) {
    // man 2 readlink says:  EINVAL The named file is not a symbolic link.
    throw InodeError(EINVAL, inodePtrFromThis(), "not a symlink");
  }

  // The symlink contents are simply the file contents!
  return readAll();
}

void FileInode::fileHandleDidClose() {
  auto state = LockedState{this};
  state->decOpenCount();
}

folly::Optional<bool> FileInode::isSameAsFast(
    const Hash& blobID,
    TreeEntryType entryType) {
  auto state = state_.rlock();
  if (entryType != treeEntryTypeFromMode(getMetadataLocked(*state).mode)) {
    return false;
  }

  if (state->hash.hasValue()) {
    // This file is not materialized, so we can compare blob hashes.
    // If the hashes are the same then assume the contents are the same.
    //
    // Unfortunately we cannot assume that the file contents are different if
    // the hashes are different: Mercurial's blob hashes also include history
    // metadata, so there may be multiple different blob hashes for the same
    // file contents.
    if (state->hash.value() == blobID) {
      return true;
    }
  }
  return folly::none;
}

folly::Future<bool> FileInode::isSameAs(
    const Blob& blob,
    TreeEntryType entryType) {
  auto result = isSameAsFast(blob.getHash(), entryType);
  if (result.hasValue()) {
    return result.value();
  }

  auto blobSha1 = Hash::sha1(&blob.getContents());
  return getSha1().thenValue(
      [blobSha1](const Hash& sha1) { return sha1 == blobSha1; });
}

folly::Future<bool> FileInode::isSameAs(
    const Hash& blobID,
    TreeEntryType entryType) {
  auto result = isSameAsFast(blobID, entryType);
  if (result.hasValue()) {
    return makeFuture(result.value());
  }

  auto f1 = getSha1();
  auto f2 = getMount()->getObjectStore()->getSha1(blobID);
  return folly::collect(f1, f2).thenValue([](std::tuple<Hash, Hash>&& result) {
    return std::get<0>(result) == std::get<1>(result);
  });
}

mode_t FileInode::getMode() const {
  return getMetadata().mode;
}

mode_t FileInode::getPermissions() const {
  return (getMode() & 07777);
}

InodeMetadata FileInode::getMetadata() const {
  auto lock = state_.rlock();
  return getMetadataLocked(*lock);
}

folly::Optional<Hash> FileInode::getBlobHash() const {
  return state_.rlock()->hash;
}

folly::Future<std::shared_ptr<FileHandle>> FileInode::open(int flags) {
  if (dtype_t::Symlink == getType()) {
    // Linux reports ELOOP if you try to open a symlink with O_NOFOLLOW set.
    // Since it isn't clear whether FUSE will allow this to happen, this
    // is a speculative defense against that happening; the O_PATH flag
    // does allow a file handle to be opened on a symlink on Linux,
    // but does not allow it to be used for real IO operations.  We're
    // punting on handling those situations here for now.
    throw InodeError(ELOOP, inodePtrFromThis(), "is a symlink");
  }

  std::shared_ptr<EdenFileHandle> fileHandle;
  {
    auto state = LockedState{this};
    state.createHandleInOuterScope(inodePtrFromThis(), &fileHandle);

    if (flags & O_TRUNC) {
      // Use truncateAndRun() to truncate the file, materializing it first if
      // necessary.  We don't actually need to run anything, so we pass in a
      // no-op lambda.
      (void)truncateAndRun(std::move(state), [](LockedState&&) { return 0; });
    } else if (flags & (O_RDWR | O_WRONLY | O_CREAT)) {
      // Call runWhileMaterialized() to begin materializing the data into the
      // overlay, since the caller will likely want to use it soon since they
      // have just opened a file handle.
      //
      // We don't wait for this to return, though, and we return the file
      // handle immediately.
      //
      // Since we just want to materialize the file and don't need to do
      // anything else we pass in a no-op lambda function.
      (void)runWhileMaterialized(
          std::move(state), [](LockedState&&) { return 0; });
    }
  }

  return fileHandle;
}

void FileInode::materializeInParent() {
  auto renameLock = getMount()->acquireRenameLock();
  auto loc = getLocationInfo(renameLock);
  if (loc.parent && !loc.unlinked) {
    loc.parent->childMaterialized(renameLock, loc.name);
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

  return getSha1().thenValue([](Hash hash) { return hash.toString(); });
}

Future<Hash> FileInode::getSha1() {
  auto state = LockedState{this};

  switch (state->tag) {
    case State::NOT_LOADED:
    case State::BLOB_LOADING:
    case State::BLOB_LOADED:
      // If a file is not materialized it should have a hash value.
      return getObjectStore()->getSha1(state->hash.value());
    case State::MATERIALIZED_IN_OVERLAY:
      state.ensureFileOpen(this);
      if (state->sha1Valid) {
        auto shaStr = fgetxattr(state->file.fd(), kXattrSha1);
        if (!shaStr.empty()) {
          return Hash(shaStr);
        }
      }
      return recomputeAndStoreSha1(state);
  }

  XLOG(FATAL) << "FileInode in illegal state: " << state->tag;
}

folly::Future<struct stat> FileInode::stat() {
  auto st = getMount()->initStatData();
  st.st_nlink = 1; // Eden does not support hard links yet.
  st.st_ino = getNodeId().get();
  // NOTE: we don't set rdev to anything special here because we
  // don't support committing special device nodes.

  auto state = LockedState{this};

  getMetadataLocked(*state).applyToStat(st);

  switch (state->tag) {
    case State::NOT_LOADED:
    case State::BLOB_LOADING:
    case State::BLOB_LOADED:
      CHECK(state->hash.has_value());
      // While getBlobMetadata will sometimes need to fetch a blob to compute
      // the size and SHA-1, if it's already known, use the cached metadata to
      // look up the size. This is especially a win after restarting Eden -
      // metadata can be loaded from the local cache more cheaply than
      // deserializing an entire blob.
      return getObjectStore()
          ->getBlobMetadata(*state->hash)
          .thenValue([st](const BlobMetadata& metadata) mutable {
            st.st_size = metadata.size;
            updateBlockCount(st);
            return st;
          });

    case State::MATERIALIZED_IN_OVERLAY:
      state.ensureFileOpen(this);
      // We are calling fstat only to get the size of the file.
      struct stat overlayStat;
      checkUnixError(fstat(state->file.fd(), &overlayStat));

      if (overlayStat.st_size < static_cast<off_t>(Overlay::kHeaderLength)) {
        // Truncated overlay files can sometimes occur after a hard reboot
        // where the overlay file data was not flushed to disk before the
        // system powered off.
        XLOG(ERR) << "overlay file for " << getNodeId()
                  << " is too short for header: size=" << overlayStat.st_size;
        throw InodeError(EIO, inodePtrFromThis(), "corrupt overlay file");
      }
      st.st_size = overlayStat.st_size - Overlay::kHeaderLength;
      updateBlockCount(st);
      return st;
  }
}

void FileInode::updateBlockCount(struct stat& st) {
  // Compute a value to store in st_blocks based on st_size.
  // Note that st_blocks always refers to 512 byte blocks, regardless of the
  // value we report in st.st_blksize.
  static constexpr off_t kBlockSize = 512;
  st.st_blocks = ((st.st_size + kBlockSize - 1) / kBlockSize);
}

void FileInode::fsync(bool datasync) {
  auto state = LockedState{this};
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
}

Future<string> FileInode::readAll() {
  return runWhileDataLoaded(
      LockedState{this},
      [self = inodePtrFromThis()](LockedState&& state) -> Future<string> {
        std::string result;
        switch (state->tag) {
          case State::MATERIALIZED_IN_OVERLAY: {
            // Note that this code requires a write lock on state_ because the
            // lseek() call modifies the file offset of the file descriptor.
            auto rc = lseek(state->file.fd(), Overlay::kHeaderLength, SEEK_SET);
            folly::checkUnixError(
                rc, "unable to seek in materialized FileInode");
            folly::readFile(state->file.fd(), result);
            break;
          }
          case State::BLOB_LOADED: {
            const auto& contentsBuf = state->blob->getContents();
            folly::io::Cursor cursor(&contentsBuf);
            result =
                cursor.readFixedString(contentsBuf.computeChainDataLength());
            break;
          }
          default:
            EDEN_BUG() << "neither materialized nor loaded during "
                          "runWhileDataLoaded() call";
        }

        // We want to update atime after the read operation.
        self->updateAtimeLocked(*state);
        return result;
      });
}

Future<BufVec> FileInode::read(size_t size, off_t off) {
  return runWhileDataLoaded(
      LockedState{this},
      [size, off, self = inodePtrFromThis()](LockedState&& state) {
        SCOPE_SUCCESS {
          self->updateAtimeLocked(*state);
        };

        if (state->tag == State::MATERIALIZED_IN_OVERLAY) {
          auto buf = folly::IOBuf::createCombined(size);
          auto res = ::pread(
              state->file.fd(),
              buf->writableBuffer(),
              size,
              off + Overlay::kHeaderLength);

          checkUnixError(res);
          buf->append(res);
          return BufVec{std::move(buf)};
        } else {
          // runWhileDataLoaded() ensures that the state is either
          // MATERIALIZED_IN_OVERLAY or BLOB_LOADED
          DCHECK_EQ(state->tag, State::BLOB_LOADED);
          auto buf = state->blob->getContents();
          folly::io::Cursor cursor(&buf);

          if (!cursor.canAdvance(off)) {
            // Seek beyond EOF.  Return an empty result.
            return BufVec{folly::IOBuf::wrapBuffer("", 0)};
          }

          cursor.skip(off);

          std::unique_ptr<folly::IOBuf> result;
          cursor.cloneAtMost(result, size);

          return BufVec{std::move(result)};
        }
      });
}

size_t FileInode::writeImpl(
    LockedState& state,
    const struct iovec* iov,
    size_t numIovecs,
    off_t off) {
  DCHECK_EQ(state->tag, State::MATERIALIZED_IN_OVERLAY);
  DCHECK(state->isFileOpen());

  state->sha1Valid = false;
  auto xfer =
      ::pwritev(state->file.fd(), iov, numIovecs, off + Overlay::kHeaderLength);
  checkUnixError(xfer);

  updateMtimeAndCtimeLocked(*state, getNow());

  state.unlock();

  auto myname = getPath();
  if (myname.hasValue()) {
    getMount()->getJournal().addDelta(std::make_unique<JournalDelta>(
        std::move(myname.value()), JournalDelta::CHANGED));
  }

  return xfer;
}

folly::Future<size_t> FileInode::write(BufVec&& buf, off_t off) {
  return runWhileMaterialized(
      LockedState{this},
      [buf = std::move(buf), off, self = inodePtrFromThis()](
          LockedState&& state) {
        auto vec = buf.getIov();
        return self->writeImpl(state, vec.data(), vec.size(), off);
      });
}

folly::Future<size_t> FileInode::write(folly::StringPiece data, off_t off) {
  auto state = LockedState{this};

  // If we are currently materialized we don't need to copy the input data.
  if (state->tag == State::MATERIALIZED_IN_OVERLAY) {
    struct iovec iov;
    iov.iov_base = const_cast<char*>(data.data());
    iov.iov_len = data.size();
    return writeImpl(state, &iov, 1, off);
  }

  return runWhileMaterialized(
      std::move(state),
      [data = data.str(), off, self = inodePtrFromThis()](
          LockedState&& stateLock) {
        struct iovec iov;
        iov.iov_base = const_cast<char*>(data.data());
        iov.iov_len = data.size();
        return self->writeImpl(stateLock, &iov, 1, off);
      });
}

Future<FileInode::FileHandlePtr> FileInode::startLoadingData(
    LockedState state) {
  DCHECK_EQ(state->tag, State::NOT_LOADED);

  // Start the blob load first in case this throws an exception.
  // Ideally the state transition is no-except in tandem with the
  // Future's .then call.
  auto blobFuture = getObjectStore()->getBlob(state->hash.value());

  // Everything from here through blobFuture.then should be noexcept.
  state->blobLoadingPromise.emplace();
  auto resultFuture = state->blobLoadingPromise->getFuture();
  state->tag = State::BLOB_LOADING;

  // Unlock state_ while we wait on the blob data to load
  state.unlock();

  auto self = inodePtrFromThis(); // separate line for formatting
  std::move(blobFuture)
      .thenTry([self](folly::Try<std::shared_ptr<const Blob>> tryBlob) mutable {
        auto state = LockedState{self};

        switch (state->tag) {
          // Since the load doesn't hold the state lock for its duration,
          // sanity check that the inode is still in loading state.
          //
          // Note that someone else may have grabbed the lock before us and
          // materialized the FileInode, so we may already be
          // MATERIALIZED_IN_OVERLAY at this point.
          case State::BLOB_LOADING: {
            auto promise = std::move(*state->blobLoadingPromise);
            state->blobLoadingPromise.clear();

            if (tryBlob.hasValue()) {
              // Transition to 'loaded' state.
              state.incOpenCount();
              state->blob = std::move(tryBlob.value());
              state->tag = State::BLOB_LOADED;
              promise.setValue(state.unlockAndCreateHandle(std::move(self)));
            } else {
              state->tag = State::NOT_LOADED;
              // Call the Future's subscribers while the state_ lock is not
              // held. Even if the FileInode has transitioned to a materialized
              // state, any pending loads must be unblocked.
              state.unlock();
              promise.setException(tryBlob.exception());
            }
            break;
          }

          case State::MATERIALIZED_IN_OVERLAY:
            // The load raced with a someone materializing the file to truncate
            // it.  Nothing left to do here.
            break;

          default:
            EDEN_BUG()
                << "Inode left in unexpected state after getBlob() completed";
        }
      })
      .thenError([](folly::exception_wrapper&&) {
        // We get here if EDEN_BUG() didn't terminate the process, or if we
        // threw in the preceding block.  Both are bad because we won't
        // automatically propagate the exception to resultFuture and we
        // can't trust the state of anything if we get here.
        // Rather than leaving something hanging, we suicide.
        // We could probably do a bit better with the error handling here :-/
        XLOG(FATAL)
            << "Failed to propagate failure in getBlob(), no choice but to die";
      });
  return resultFuture;
}

void FileInode::materializeNow(LockedState& state) {
  // This function should only be called from the BLOB_LOADED state
  DCHECK_EQ(state->tag, State::BLOB_LOADED);
  CHECK(state->blob);

  // Look up the blob metadata so we can get the blob contents SHA1
  // Since this uses state->hash we perform this before calling
  // state.setMaterialized()
  auto blobSha1 = getObjectStore()->getSha1(state->hash.value());

  auto timestamps = getMetadataLocked(*state).timestamps;

  auto file = getMount()->getOverlay()->createOverlayFile(
      getNodeId(), timestamps, state->blob->getContents());
  state.setMaterialized(std::move(file));

  // If we have a SHA-1 from the metadata, apply it to the new file.  This
  // saves us from recomputing it again in the case that something opens the
  // file read/write and closes it without changing it.
  if (blobSha1.isReady()) {
    storeSha1(state, blobSha1.value());
  } else {
    // Leave the SHA-1 attribute dirty - it is not very likely that a file
    // will be opened for writing, closed without changing, and then have
    // its SHA-1 queried via Thrift or xattr. If so, the SHA-1 will be
    // recomputed as needed. That said, it's perhaps cheaper to hash now
    // (SHA-1 is hundreds of MB/s) while the data is accessible in the blob
    // than to read the file out of the overlay later.
  }
}

void FileInode::materializeAndTruncate(LockedState& state) {
  CHECK_NE(state->tag, State::MATERIALIZED_IN_OVERLAY);
  auto timestamps = getMetadataLocked(*state).timestamps;
  auto file = getMount()->getOverlay()->createOverlayFile(
      getNodeId(), timestamps, ByteRange{});
  state.setMaterialized(std::move(file));
  storeSha1(state, Hash::sha1(ByteRange{}));
}

void FileInode::truncateInOverlay(LockedState& state) {
  CHECK_EQ(state->tag, State::MATERIALIZED_IN_OVERLAY);
  CHECK(!state->hash);
  CHECK(!state->blob);

  state.ensureFileOpen(this);
  checkUnixError(ftruncate(state->file.fd(), 0 + Overlay::kHeaderLength));
}

ObjectStore* FileInode::getObjectStore() const {
  return getMount()->getObjectStore();
}

Hash FileInode::recomputeAndStoreSha1(const LockedState& state) {
  DCHECK_EQ(state->tag, State::MATERIALIZED_IN_OVERLAY);
  DCHECK(state->isFileOpen());

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

void FileInode::storeSha1(const LockedState& state, Hash sha1) {
  DCHECK_EQ(state->tag, State::MATERIALIZED_IN_OVERLAY);
  DCHECK(state->isFileOpen());

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

folly::Future<folly::Unit> FileInode::prefetch() {
  // Careful to only hold the lock while fetching a copy of the hash.
  return folly::via(getMount()->getThreadPool().get())
      .thenValue([this](auto&&) {
        if (auto hash = state_.rlock()->hash) {
          getObjectStore()->getBlobMetadata(*hash);
        }
      });
}

} // namespace eden
} // namespace facebook
