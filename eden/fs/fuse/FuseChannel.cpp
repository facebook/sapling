/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/FuseChannel.h"

#include <boost/cast.hpp>
#include <folly/futures/Future.h>
#include <folly/io/async/Request.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <signal.h>
#include <type_traits>
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Synchronized.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;
using std::string;

namespace facebook {
namespace eden {

namespace {

// This is the minimum size used by libfuse so we use it too!
constexpr size_t MIN_BUFSIZE = 0x21000;

StringPiece fuseOpcodeName(FuseOpcode opcode) {
  switch (opcode) {
    case FUSE_LOOKUP:
      return "FUSE_LOOKUP";
    case FUSE_FORGET:
      return "FUSE_FORGET";
    case FUSE_GETATTR:
      return "FUSE_GETATTR";
    case FUSE_SETATTR:
      return "FUSE_SETATTR";
    case FUSE_READLINK:
      return "FUSE_READLINK";
    case FUSE_SYMLINK:
      return "FUSE_SYMLINK";
    case FUSE_MKNOD:
      return "FUSE_MKNOD";
    case FUSE_MKDIR:
      return "FUSE_MKDIR";
    case FUSE_UNLINK:
      return "FUSE_UNLINK";
    case FUSE_RMDIR:
      return "FUSE_RMDIR";
    case FUSE_RENAME:
      return "FUSE_RENAME";
    case FUSE_LINK:
      return "FUSE_LINK";
    case FUSE_OPEN:
      return "FUSE_OPEN";
    case FUSE_READ:
      return "FUSE_READ";
    case FUSE_WRITE:
      return "FUSE_WRITE";
    case FUSE_STATFS:
      return "FUSE_STATFS";
    case FUSE_RELEASE:
      return "FUSE_RELEASE";
    case FUSE_FSYNC:
      return "FUSE_FSYNC";
    case FUSE_SETXATTR:
      return "FUSE_SETXATTR";
    case FUSE_GETXATTR:
      return "FUSE_GETXATTR";
    case FUSE_LISTXATTR:
      return "FUSE_LISTXATTR";
    case FUSE_REMOVEXATTR:
      return "FUSE_REMOVEXATTR";
    case FUSE_FLUSH:
      return "FUSE_FLUSH";
    case FUSE_INIT:
      return "FUSE_INIT";
    case FUSE_OPENDIR:
      return "FUSE_OPENDIR";
    case FUSE_READDIR:
      return "FUSE_READDIR";
    case FUSE_RELEASEDIR:
      return "FUSE_RELEASEDIR";
    case FUSE_FSYNCDIR:
      return "FUSE_FSYNCDIR";
    case FUSE_GETLK:
      return "FUSE_GETLK";
    case FUSE_SETLK:
      return "FUSE_SETLK";
    case FUSE_SETLKW:
      return "FUSE_SETLKW";
    case FUSE_ACCESS:
      return "FUSE_ACCESS";
    case FUSE_CREATE:
      return "FUSE_CREATE";
    case FUSE_INTERRUPT:
      return "FUSE_INTERRUPT";
    case FUSE_BMAP:
      return "FUSE_BMAP";
    case FUSE_DESTROY:
      return "FUSE_DESTROY";
    case FUSE_IOCTL:
      return "FUSE_IOCTL";
    case FUSE_POLL:
      return "FUSE_POLL";
    case FUSE_NOTIFY_REPLY:
      return "FUSE_NOTIFY_REPLY";
    case FUSE_BATCH_FORGET:
      return "FUSE_BATCH_FORGET";
    case FUSE_FALLOCATE:
      return "FUSE_FALLOCATE";
#ifdef __linux__
    case FUSE_READDIRPLUS:
      return "FUSE_READDIRPLUS";
    case FUSE_RENAME2:
      return "FUSE_RENAME2";
    case FUSE_LSEEK:
      return "FUSE_LSEEK";
#endif
#ifdef __APPLE__
    case FUSE_SETVOLNAME:
      return "FUSE_SETVOLNAME";
    case FUSE_GETXTIMES:
      return "FUSE_GETXTIMES";
    case FUSE_EXCHANGE:
      return "FUSE_EXCHANGE";
#endif

    case CUSE_INIT:
      return "CUSE_INIT";
  }
  return "<unknown>";
}

using Handler = folly::Future<folly::Unit> (
    FuseChannel::*)(const fuse_in_header* header, const uint8_t* arg);

void sigusr2Handler(int /* signum */) {
  // Do nothing.
  // The purpose of this signal is only to interrupt the blocking read() calls
  // in processSession() and readInitPacket()
}

void installSignalHandler() {
  // We use SIGUSR2 to wake up our worker threads when we want to shut down.
  // Install a signal handler for this signal.  The signal handler itself is a
  // no-op, we simply want to use it to interrupt blocking read() calls.
  //
  // We will re-install this handler each time a FuseChannel object is called,
  // but that should be fine.
  //
  // This must be installed using sigaction() rather than signal(), so we can
  // ensure that the SA_RESTART flag is not ste.
  struct sigaction action = {};
  action.sa_handler = sigusr2Handler;
  sigemptyset(&action.sa_mask);
  action.sa_flags = 0; // We intentionally turn off SA_RESTART
  struct sigaction oldAction;
  folly::checkUnixError(
      sigaction(SIGUSR2, &action, &oldAction), "failed to set SIGUSR2 handler");
}

} // namespace

struct FuseChannel::HandlerEntry {
  Handler handler;
  FuseThreadStats::HistogramPtr histogram;
};

const FuseChannel::HandlerMap FuseChannel::handlerMap_ = {
    {FUSE_READ, {&FuseChannel::fuseRead, &FuseThreadStats::read}},
    {FUSE_WRITE, {&FuseChannel::fuseWrite, &FuseThreadStats::write}},
    {FUSE_LOOKUP, {&FuseChannel::fuseLookup, &FuseThreadStats::lookup}},
    {FUSE_FORGET, {&FuseChannel::fuseForget, &FuseThreadStats::forget}},
    {FUSE_GETATTR, {&FuseChannel::fuseGetAttr, &FuseThreadStats::getattr}},
    {FUSE_SETATTR, {&FuseChannel::fuseSetAttr, &FuseThreadStats::setattr}},
    {FUSE_READLINK, {&FuseChannel::fuseReadLink, &FuseThreadStats::readlink}},
    {FUSE_SYMLINK, {&FuseChannel::fuseSymlink, &FuseThreadStats::symlink}},
    {FUSE_MKNOD, {&FuseChannel::fuseMknod, &FuseThreadStats::mknod}},
    {FUSE_MKDIR, {&FuseChannel::fuseMkdir, &FuseThreadStats::mkdir}},
    {FUSE_UNLINK, {&FuseChannel::fuseUnlink, &FuseThreadStats::unlink}},
    {FUSE_RMDIR, {&FuseChannel::fuseRmdir, &FuseThreadStats::rmdir}},
    {FUSE_RENAME, {&FuseChannel::fuseRename, &FuseThreadStats::rename}},
    {FUSE_LINK, {&FuseChannel::fuseLink, &FuseThreadStats::link}},
    {FUSE_OPEN, {&FuseChannel::fuseOpen, &FuseThreadStats::open}},
    {FUSE_STATFS, {&FuseChannel::fuseStatFs, &FuseThreadStats::statfs}},
    {FUSE_RELEASE, {&FuseChannel::fuseRelease, &FuseThreadStats::release}},
    {FUSE_FSYNC, {&FuseChannel::fuseFsync, &FuseThreadStats::fsync}},
    {FUSE_SETXATTR, {&FuseChannel::fuseSetXAttr, &FuseThreadStats::setxattr}},
    {FUSE_GETXATTR, {&FuseChannel::fuseGetXAttr, &FuseThreadStats::getxattr}},
    {FUSE_LISTXATTR,
     {&FuseChannel::fuseListXAttr, &FuseThreadStats::listxattr}},
    {FUSE_REMOVEXATTR,
     {&FuseChannel::fuseRemoveXAttr, &FuseThreadStats::removexattr}},
    {FUSE_FLUSH, {&FuseChannel::fuseFlush, &FuseThreadStats::flush}},
    {FUSE_OPENDIR, {&FuseChannel::fuseOpenDir, &FuseThreadStats::opendir}},
    {FUSE_READDIR, {&FuseChannel::fuseReadDir, &FuseThreadStats::readdir}},
    {FUSE_RELEASEDIR,
     {&FuseChannel::fuseReleaseDir, &FuseThreadStats::releasedir}},
    {FUSE_FSYNCDIR, {&FuseChannel::fuseFsyncDir, &FuseThreadStats::fsyncdir}},
    {FUSE_ACCESS, {&FuseChannel::fuseAccess, &FuseThreadStats::access}},
    {FUSE_CREATE, {&FuseChannel::fuseCreate, &FuseThreadStats::create}},
    {FUSE_BMAP, {&FuseChannel::fuseBmap, &FuseThreadStats::bmap}},
    {FUSE_BATCH_FORGET,
     {&FuseChannel::fuseBatchForget, &FuseThreadStats::forgetmulti}},
};

static iovec inline make_iovec(const void* addr, size_t len) {
  iovec iov;
  iov.iov_base = const_cast<void*>(addr);
  iov.iov_len = len;
  return iov;
}

template <typename T>
static iovec inline make_iovec(const T& t) {
  return make_iovec(&t, sizeof(t));
}

static std::string flagsToLabel(
    const std::unordered_map<int32_t, const char*>& labels,
    uint32_t flags) {
  std::vector<const char*> bits;
  for (const auto& it : labels) {
    if (it.first == 0) {
      // Sometimes a define evaluates to zero; it's not useful so skip it
      continue;
    }
    if ((static_cast<decltype(it.first)>(flags) & it.first) == it.first) {
      bits.push_back(it.second);
      flags &= ~it.first;
    }
  }
  std::string str;
  folly::join(" ", bits, str);
  if (flags == 0) {
    return str;
  }
  return folly::format("{} unknown:0x{:x}", str, flags).str();
}

static const std::unordered_map<int32_t, const char*> capsLabels = {
    {FUSE_ASYNC_READ, "ASYNC_READ"},
    {FUSE_POSIX_LOCKS, "POSIX_LOCKS"},
    {FUSE_ATOMIC_O_TRUNC, "ATOMIC_O_TRUNC"},
    {FUSE_EXPORT_SUPPORT, "EXPORT_SUPPORT"},
    {FUSE_BIG_WRITES, "BIG_WRITES"},
    {FUSE_DONT_MASK, "DONT_MASK"},
    {FUSE_FLOCK_LOCKS, "FLOCK_LOCKS"},
#ifdef __linux__
    {FUSE_SPLICE_WRITE, "SPLICE_WRITE"},
    {FUSE_SPLICE_MOVE, "SPLICE_MOVE"},
    {FUSE_SPLICE_READ, "SPLICE_READ"},
    {FUSE_HAS_IOCTL_DIR, "IOCTL_DIR"},
    {FUSE_AUTO_INVAL_DATA, "AUTO_INVAL_DATA"},
    {FUSE_DO_READDIRPLUS, "DO_READDIRPLUS"},
    {FUSE_READDIRPLUS_AUTO, "READDIRPLUS_AUTO"},
    {FUSE_ASYNC_DIO, "ASYNC_DIO"},
    {FUSE_WRITEBACK_CACHE, "WRITEBACK_CACHE"},
    {FUSE_NO_OPEN_SUPPORT, "NO_OPEN_SUPPORT"},
    {FUSE_PARALLEL_DIROPS, "PARALLEL_DIROPS"},
    {FUSE_HANDLE_KILLPRIV, "HANDLE_KILLPRIV"},
    {FUSE_POSIX_ACL, "POSIX_ACL"},
    {FUSE_CACHE_SYMLINKS, "CACHE_SYMLINKS"},
    {FUSE_NO_OPENDIR_SUPPORT, "NO_OPENDIR_SUPPORT"},
#endif
#ifdef __APPLE__
    {FUSE_ALLOCATE, "ALLOCATE"},
    {FUSE_EXCHANGE_DATA, "EXCHANGE_DATA"},
    {FUSE_CASE_INSENSITIVE, "CASE_INSENSITIVE"},
    {FUSE_VOL_RENAME, "VOL_RENAME"},
    {FUSE_XTIMES, "XTIMES"},
#endif
};

FuseChannel::DataRange::DataRange(int64_t off, int64_t len)
    : offset(off), length(len) {}

FuseChannel::InvalidationEntry::InvalidationEntry(
    InodeNumber num,
    PathComponentPiece n)
    : type(InvalidationType::DIR_ENTRY), inode(num), name(n) {}

FuseChannel::InvalidationEntry::InvalidationEntry(
    InodeNumber num,
    int64_t offset,
    int64_t length)
    : type(InvalidationType::INODE), inode(num), range(offset, length) {}

FuseChannel::InvalidationEntry::InvalidationEntry(Promise<Unit> p)
    : type(InvalidationType::FLUSH),
      inode(kRootNodeId),
      promise(std::move(p)) {}

FuseChannel::InvalidationEntry::~InvalidationEntry() {
  switch (type) {
    case InvalidationType::INODE:
      range.~DataRange();
      return;
    case InvalidationType::DIR_ENTRY:
      name.~PathComponent();
      return;
    case InvalidationType::FLUSH:
      promise.~Promise();
      return;
  }
  XLOG(FATAL) << "unknown InvalidationEntry type: "
              << static_cast<uint64_t>(type);
}

FuseChannel::InvalidationEntry::InvalidationEntry(
    InvalidationEntry&& other) noexcept
    : type(other.type), inode(other.inode) {
  // For simplicity we just declare the InvalidationEntry move constructor as
  // unconditionally noexcept in FuseChannel.h
  // Assert that this is actually true.
  static_assert(
      std::is_nothrow_move_constructible<PathComponent>::value,
      "All members should be nothrow move constructible");
  static_assert(
      std::is_nothrow_move_constructible<Promise<Unit>>::value,
      "All members should be nothrow move constructible");
  static_assert(
      std::is_nothrow_move_constructible<DataRange>::value,
      "All members should be nothrow move constructible");

  switch (type) {
    case InvalidationType::INODE:
      new (&range) DataRange(std::move(other.range));
      return;
    case InvalidationType::DIR_ENTRY:
      new (&name) PathComponent(std::move(other.name));
      return;
    case InvalidationType::FLUSH:
      new (&promise) Promise<Unit>(std::move(other.promise));
      return;
  }
}

std::ostream& operator<<(
    std::ostream& os,
    const FuseChannel::InvalidationEntry& entry) {
  switch (entry.type) {
    case FuseChannel::InvalidationType::INODE:
      return os << "(inode " << entry.inode << ", offset " << entry.range.offset
                << ", length " << entry.range.length << ")";
    case FuseChannel::InvalidationType::DIR_ENTRY:
      return os << "(inode " << entry.inode << ", child \"" << entry.name
                << "\")";
    case FuseChannel::InvalidationType::FLUSH:
      return os << "(invalidation flush)";
  }
  return os << "(unknown invalidation type "
            << static_cast<uint64_t>(entry.type) << " inode " << entry.inode
            << ")";
}

void FuseChannel::replyError(const fuse_in_header& request, int errorCode) {
  fuse_out_header err;
  err.len = sizeof(err);
  err.error = -errorCode;
  err.unique = request.unique;
  XLOG(DBG7) << "replyError unique=" << err.unique << " error=" << errorCode
             << " " << folly::errnoStr(errorCode);
  auto res = write(fuseDevice_.fd(), &err, sizeof(err));
  if (res != sizeof(err)) {
    if (res < 0) {
      throwSystemError("replyError: error writing to fuse device");
    } else {
      throw std::runtime_error("unexpected short write to FUSE device");
    }
  }
}

void FuseChannel::sendReply(
    const fuse_in_header& request,
    folly::fbvector<iovec>&& vec) const {
  fuse_out_header out;
  out.unique = request.unique;
  out.error = 0;

  vec.insert(vec.begin(), make_iovec(out));

  sendRawReply(vec.data(), vec.size());
}

void FuseChannel::sendReply(
    const fuse_in_header& request,
    folly::ByteRange bytes) const {
  fuse_out_header out;
  out.unique = request.unique;
  out.error = 0;

  std::array<iovec, 2> iov;
  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);
  iov[1].iov_base = const_cast<uint8_t*>(bytes.data());
  iov[1].iov_len = bytes.size();

  sendRawReply(iov.data(), iov.size());
}

void FuseChannel::sendRawReply(const iovec iov[], size_t count) const {
  // Ensure that the length is set correctly
  DCHECK_EQ(iov[0].iov_len, sizeof(fuse_out_header));
  const auto header = reinterpret_cast<fuse_out_header*>(iov[0].iov_base);
  header->len = 0;
  for (size_t i = 0; i < count; ++i) {
    header->len += iov[i].iov_len;
  }

  const auto res = writev(fuseDevice_.fd(), iov, count);
  const int err = errno;
  XLOG(DBG7) << "sendRawReply: unique=" << header->unique
             << " header->len=" << header->len << " wrote=" << res;

  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal.  We don't need to log this,
      // but will propagate it back to our caller.
    } else if (!isFuseDeviceValid(state_.rlock()->stopReason)) {
      XLOG(INFO) << "error writing to fuse device: session closed";
    } else {
      XLOG(WARNING) << "error writing to fuse device: " << folly::errnoStr(err);
    }
    throwSystemErrorExplicit(err, "error writing to fuse device");
  }
}

FuseChannel::FuseChannel(
    folly::File&& fuseDevice,
    AbsolutePathPiece mountPath,
    size_t numThreads,
    Dispatcher* const dispatcher,
    std::shared_ptr<ProcessNameCache> processNameCache,
    folly::Duration requestTimeout)
    : bufferSize_(std::max(size_t(getpagesize()) + 0x1000, MIN_BUFSIZE)),
      numThreads_(numThreads),
      dispatcher_(dispatcher),
      mountPath_(mountPath),
      requestTimeout_(requestTimeout),
      fuseDevice_(std::move(fuseDevice)),
      processAccessLog_(std::move(processNameCache)) {
  CHECK_GE(numThreads_, 1);
  installSignalHandler();
}

FuseChannel::~FuseChannel() {}

Future<FuseChannel::StopFuture> FuseChannel::initialize() {
  // Start one worker thread which will perform the initialization,
  // and will then start the remaining worker threads and signal success
  // once initialization completes.
  return folly::makeFutureWith([&] {
    auto state = state_.wlock();
    state->workerThreads.reserve(numThreads_);
    state->workerThreads.emplace_back([this] { initWorkerThread(); });
    return initPromise_.getFuture();
  });
}

FuseChannel::StopFuture FuseChannel::initializeFromTakeover(
    fuse_init_out connInfo) {
  connInfo_ = connInfo;
  dispatcher_->initConnection(connInfo);
  XLOG(DBG1) << "Takeover using max_write=" << connInfo_->max_write
             << ", max_readahead=" << connInfo_->max_readahead
             << ", want=" << flagsToLabel(capsLabels, connInfo_->flags);
  startWorkerThreads();
  return sessionCompletePromise_.getFuture();
}

void FuseChannel::startWorkerThreads() {
  auto state = state_.wlock();

  // After aquiring the state_ lock check to see if we have been asked to shut
  // down.  If so just return without doing anything.
  //
  // This can happen if the FuseChannel is destroyed very shortly after we
  // finish processing the INIT request.  In this case we don't want to start
  // the remaining worker threads if the destructor is trying to stop and join
  // them.
  if (state->stopReason != StopReason::RUNNING) {
    return;
  }

  try {
    state->workerThreads.reserve(numThreads_);
    while (state->workerThreads.size() < numThreads_) {
      state->workerThreads.emplace_back([this] { fuseWorkerThread(); });
    }

    invalidationThread_ = std::thread([this] { invalidationThread(); });
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error starting FUSE worker threads: " << exceptionStr(ex);
    // Request any threads we did start to stop now.
    requestSessionExit(state, StopReason::INIT_FAILED);
    stopInvalidationThread();
    throw;
  }
}

void FuseChannel::destroy() {
  std::vector<std::thread> threads;
  {
    auto state = state_.wlock();
    requestSessionExit(state, StopReason::DESTRUCTOR);
    threads.swap(state->workerThreads);
  }

  for (auto& thread : threads) {
    if (std::this_thread::get_id() == thread.get_id()) {
      XLOG(FATAL) << "cannot destroy a FuseChannel from inside one of "
                     "its own worker threads";
    }
    thread.join();
  }

  // Check to see if there are still outstanding requests.
  // If so, delay actual deletion of the FuseChannel object until the
  // last request completes.
  bool allDone = false;
  {
    auto state = state_.wlock();
    if (state->requests.empty()) {
      allDone = true;
    } else {
      state->destroyPending = true;
    }
  }
  if (allDone) {
    delete this;
  }
}

void FuseChannel::invalidateInode(InodeNumber ino, off_t off, off_t len) {
  // Add the entry to invalidationQueue_ and wake up the invalidation thread to
  // send it.
  invalidationQueue_.lock()->queue.emplace_back(ino, off, len);
  invalidationCV_.notify_one();
}

void FuseChannel::invalidateEntry(InodeNumber parent, PathComponentPiece name) {
  // Add the entry to invalidationQueue_ and wake up the invalidation thread to
  // send it.
  invalidationQueue_.lock()->queue.emplace_back(parent, name);
  invalidationCV_.notify_one();
}

void FuseChannel::invalidateInodes(folly::Range<InodeNumber*> range) {
  {
    auto queue = invalidationQueue_.lock();
    std::transform(
        range.begin(),
        range.end(),
        std::back_insert_iterator(queue->queue),
        [](const auto& inodeNum) { return InvalidationEntry(inodeNum, 0, 0); });
  }
  if (range.begin() != range.end()) {
    invalidationCV_.notify_one();
  }
}
folly::Future<folly::Unit> FuseChannel::flushInvalidations() {
  // Add a promise to the invalidation queue, which the invalidation thread
  // will fulfill once it reaches that element in the queue.
  Promise<Unit> promise;
  auto result = promise.getFuture();
  invalidationQueue_.lock()->queue.emplace_back(std::move(promise));
  invalidationCV_.notify_one();
  return result;
}

/**
 * Send an element from the invalidation queue.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidation(InvalidationEntry& entry) {
  // We catch any exceptions that occur and simply log an error message.
  // There is not much else we can do in this situation.
  XLOG(DBG6) << "sending invalidation request: " << entry;
  try {
    switch (entry.type) {
      case InvalidationType::INODE:
        sendInvalidateInode(
            entry.inode, entry.range.offset, entry.range.length);
        return;
      case InvalidationType::DIR_ENTRY:
        sendInvalidateEntry(entry.inode, entry.name);
        return;
      case InvalidationType::FLUSH:
        // Fulfill the promise to indicate that all previous entries in the
        // invalidation queue have been completed.
        entry.promise.setValue();
        return;
    }
    EDEN_BUG() << "unknown invalidation entry type "
               << static_cast<uint64_t>(entry.type);
  } catch (const std::system_error& ex) {
    // Log ENOENT errors as a debug message.  This can happen for inode numbers
    // that we allocated on our own and haven't actually told the kernel about
    // yet.
    if (isEnoent(ex)) {
      XLOG(DBG3) << "received ENOENT when sending invalidation request: "
                 << entry;
    } else {
      XLOG(ERR) << "error sending invalidation request: " << entry << ": "
                << folly::exceptionStr(ex);
    }
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error sending invalidation request: " << entry << ": "
              << folly::exceptionStr(ex);
  }
}

/**
 * Send a FUSE_NOTIFY_INVAL_INODE message to the kernel.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidateInode(
    InodeNumber ino,
    int64_t off,
    int64_t len) {
  XLOG(DBG3) << "sendInvalidateInode(ino=" << ino << ", off=" << off
             << ", len=" << len << ")";
  fuse_notify_inval_inode_out notify;
  notify.ino = ino.get();
  notify.off = off;
  notify.len = len;

  fuse_out_header out;
  out.unique = 0;
  out.error = FUSE_NOTIFY_INVAL_INODE;

  std::array<iovec, 2> iov;

  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);

  iov[1].iov_base = &notify;
  iov[1].iov_len = sizeof(notify);

  try {
    sendRawReply(iov.data(), iov.size());
    XLOG(DBG7) << "sendInvalidateInode(ino=" << ino << ", off=" << off
               << ", len=" << len << ") OK!";
  } catch (const std::system_error& exc) {
    // Ignore ENOENT.  This can happen for inode numbers that we allocated on
    // our own and haven't actually told the kernel about yet.
    if (!isEnoent(exc)) {
      XLOG(ERR) << "sendInvalidateInode(ino=" << ino << ", off=" << off
                << ", len=" << len << ") failed: " << exc.what();
      throwSystemErrorExplicit(
          exc.code().value(), "error invalidating FUSE inode ", ino);
    } else {
      XLOG(DBG3) << "sendInvalidateInode(ino=" << ino << ", off=" << off
                 << ", len=" << len << ") failed with ENOENT";
    }
  }
}

/**
 * Send a FUSE_NOTIFY_INVAL_ENTRY message to the kernel.
 *
 * This method always runs in the invalidation thread.
 */
void FuseChannel::sendInvalidateEntry(
    InodeNumber parent,
    PathComponentPiece name) {
  XLOG(DBG3) << "sendInvalidateEntry(parent=" << parent << ", name=" << name
             << ")";

  auto namePiece = name.stringPiece();

  fuse_notify_inval_entry_out notify = {};
  notify.parent = parent.get();
  notify.namelen = namePiece.size();

  fuse_out_header out;
  out.unique = 0;
  out.error = FUSE_NOTIFY_INVAL_ENTRY;

  std::array<iovec, 4> iov;

  iov[0].iov_base = &out;
  iov[0].iov_len = sizeof(out);

  iov[1].iov_base = &notify;
  iov[1].iov_len = sizeof(notify);

  iov[2].iov_base = const_cast<char*>(namePiece.data());
  iov[2].iov_len = namePiece.size();

  // libfuse adds an extra 1 count to the size that it sends to the kernel,
  // presumably because it is assuming that the string is already NUL
  // terminated.  That is misleading because the API provides a size parameter
  // that implies that the string doesn't require termination.  We deal with
  // this more safely here by adding a vec element holding a NUL byte.
  iov[3].iov_base = const_cast<char*>("\x00");
  iov[3].iov_len = 1;

  try {
    sendRawReply(iov.data(), iov.size());
  } catch (const std::system_error& exc) {
    // Ignore ENOENT.  This can happen for inode numbers that we allocated on
    // our own and haven't actually told the kernel about yet.
    if (!isEnoent(exc)) {
      throwSystemErrorExplicit(
          exc.code().value(),
          "error invalidating FUSE entry ",
          name,
          " in directory inode ",
          parent);
    } else {
      XLOG(DBG3) << "sendInvalidateEntry(parent=" << parent << ", name=" << name
                 << ") failed with ENOENT";
    }
  }
}

std::vector<fuse_in_header> FuseChannel::getOutstandingRequests() {
  auto state = state_.wlock();
  const auto& requests = state->requests;
  std::vector<fuse_in_header> outstandingCalls;

  for (const auto& entry : requests) {
    auto ctx = entry.second.lock();
    if (ctx) {
      // Get the fuse_in_header from the ctx and push a copy of it on the
      // outstandingCalls collection
      auto rdata = boost::polymorphic_downcast<RequestData*>(
          ctx->getContextData(RequestData::kKey));
      // rdata should never be null here and if it - it's most likely a bug
      const fuse_in_header& fuseHeader = rdata->examineReq();
      if (fuseHeader.opcode != 0) {
        outstandingCalls.push_back(fuseHeader);
      }
    }
  }
  return outstandingCalls;
}

void FuseChannel::requestSessionExit(StopReason reason) {
  requestSessionExit(state_.wlock(), reason);
}

void FuseChannel::requestSessionExit(
    const Synchronized<State>::LockedPtr& state,
    StopReason reason) {
  // We have already been asked to stop before.
  if (state->stopReason != StopReason::RUNNING) {
    // Update state->stopReason only if the old stop reason left the FUSE
    // device in a still usable state but the new reason does not.
    if (isFuseDeviceValid(state->stopReason) &&
        !isFuseDeviceValid(state->stopReason)) {
      state->stopReason = reason;
    }
    return;
  }

  // This was the first time requestSessionExit has been called.
  // Record the reason we are stopping and then notify worker threads to
  // stop.
  state->stopReason = reason;

  // Update stop_ so that worker threads will break out of their loop.
  stop_.store(true, std::memory_order_relaxed);

  // Send a signal to knock our workers out of their blocking read() syscalls
  // TODO: This code is slightly racy, since threads could receive the signal
  // immediately before entering read().  In the long run it would be nicer to
  // have the worker threads use epoll and then use an eventfd to signal them
  // to stop.
  for (auto& thr : state->workerThreads) {
    if (thr.joinable() && thr.get_id() != std::this_thread::get_id()) {
      pthread_kill(thr.native_handle(), SIGUSR2);
    }
  }
}

void FuseChannel::setThreadSigmask() {
  // Make sure our thread will receive SIGUSR2
  sigset_t sigset;
  sigemptyset(&sigset);
  sigaddset(&sigset, SIGUSR2);

  sigset_t oldset;
  sigemptyset(&oldset);

  folly::checkPosixError(pthread_sigmask(SIG_UNBLOCK, &sigset, &oldset));
}

void FuseChannel::initWorkerThread() noexcept {
  try {
    setThreadSigmask();
    setThreadName(to<std::string>("fuse", mountPath_.basename()));

    // Read the INIT packet
    readInitPacket();

    // Start the other FUSE worker threads.
    startWorkerThreads();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error performing FUSE channel initialization: "
              << exceptionStr(ex);
    // Indicate that initialization failed.
    initPromise_.setException(
        folly::exception_wrapper(std::current_exception(), ex));
    return;
  }

  // Signal that initialization is complete.
  initPromise_.setValue(sessionCompletePromise_.getSemiFuture());

  // Continue to run like a normal FUSE worker thread.
  fuseWorkerThread();
}

void FuseChannel::fuseWorkerThread() noexcept {
  setThreadName(to<std::string>("fuse", mountPath_.basename()));
  setThreadSigmask();

  try {
    processSession();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "unexpected error in FUSE worker thread: " << exceptionStr(ex);
    // Request that all other FUSE threads exit.
    // This will cause us to stop processing the mount and signal our session
    // complete future.
    requestSessionExit(StopReason::WORKER_EXCEPTION);
    // Fall through and continue with the normal thread exit code.
  }

  // Record that we have shut down.
  {
    auto state = state_.wlock();
    ++state->stoppedThreads;
    DCHECK(!state->destroyPending) << "destroyPending cannot be set while "
                                      "worker threads are still running";

    // If we are the last thread to stop and there are no more requests
    // outstanding then invoke sessionComplete().  If we are the last thread
    // but there are still outstanding requests we will invoke
    // sessionComplete() when we process the final stage of the request
    // processing for the last request.
    if (state->stoppedThreads == numThreads_ && state->requests.empty()) {
      sessionComplete(std::move(state));
    }
  }
}

void FuseChannel::invalidationThread() noexcept {
  // We send all FUSE_NOTIFY_INVAL_ENTRY and FUSE_NOTIFY_INVAL_INODE requests
  // in a dedicated thread.  These requests will block in the kernel until it
  // can obtain the inode lock on the inode in question.
  //
  // It is possible that the kernel-level inode lock is already held by another
  // thread that is waiting on one of our own user-space locks.  To avoid
  // deadlock, we therefore need to make sure that we are never holding any
  // Eden locks when sending these invalidation requests.
  //
  // For example, a process calling unlink(parent_dir, "foo") will acquire the
  // inode lock for parent_dir in the kernel, and the kernel will then send an
  // unlink request to Eden.  This unlink request will require the mount
  // point's rename lock to proceed.  If a checkout is currently in progress it
  // currently owns the rename lock, and will generate invalidation requests.
  // We need to make sure the checkout operation does not block waiting on the
  // invalidation requests to complete, since otherwise this would deadlock.
  while (true) {
    // Wait for entries to process
    std::vector<InvalidationEntry> entries;
    {
      auto lockedQueue = invalidationQueue_.lock();
      while (lockedQueue->queue.empty()) {
        if (lockedQueue->stop) {
          return;
        }
        invalidationCV_.wait(lockedQueue.getUniqueLock());
      }
      lockedQueue->queue.swap(entries);
    }

    // Process all of the entries we found
    for (auto& entry : entries) {
      sendInvalidation(entry);
    }
    entries.clear();
  }
}

void FuseChannel::stopInvalidationThread() {
  // Check that the thread is joinable just in case we were destroyed
  // before the invalidation thread was started.
  if (!invalidationThread_.joinable()) {
    return;
  }

  invalidationQueue_.lock()->stop = true;
  invalidationCV_.notify_one();
  invalidationThread_.join();
}

void FuseChannel::readInitPacket() {
  struct {
    fuse_in_header header;
    fuse_init_in init;
  } init;

  // Loop until we receive the INIT packet, or until we are stopped.
  while (true) {
    if (stop_.load(std::memory_order_relaxed)) {
      throw std::runtime_error(folly::to<string>(
          "FuseChannel for \"",
          mountPath_,
          "\" stopped while waiting for INIT packet"));
    }

    auto res = read(fuseDevice_.fd(), &init, sizeof(init));
    if (res < 0) {
      int errnum = errno;
      if (stop_.load(std::memory_order_relaxed)) {
        throw std::runtime_error(folly::to<string>(
            "FuseChannel for \"",
            mountPath_,
            "\" stopped while waiting for INIT packet"));
      }

      if (errnum == EINTR || errnum == EAGAIN || errnum == ENOENT) {
        // These are all variations on being interrupted; let's
        // continue and retry.
        continue;
      }
      if (errnum == ENODEV) {
        throw FuseDeviceUnmountedDuringInitialization(mountPath_);
      }
      throw std::runtime_error(folly::to<string>(
          "error reading from FUSE device for \"",
          mountPath_,
          "\" while expecting INIT request: ",
          folly::errnoStr(errnum)));
    }
    if (res == 0) {
      // This is generally caused by the unit tests closing a fake fuse
      // channel.  When we are actually connected to the kernel we normally
      // expect to see an ENODEV error rather than EOF.
      throw FuseDeviceUnmountedDuringInitialization(mountPath_);
    }

    // Error out if the kernel sends less data than we expected.
    // We currently don't error out for now if we receive more data: maybe this
    // could happen for future kernel versions that speak a newer FUSE protocol
    // with extra fields in fuse_init_in?
    if (static_cast<size_t>(res) < sizeof(init)) {
      throw std::runtime_error(folly::to<string>(
          "received partial FUSE_INIT packet on mount \"",
          mountPath_,
          "\": size=",
          res));
    }

    break;
  }

  if (init.header.opcode != FUSE_INIT) {
    replyError(init.header, EPROTO);
    throw std::runtime_error(folly::to<std::string>(
        "expected to receive FUSE_INIT for \"",
        mountPath_,
        "\" but got ",
        fuseOpcodeName(init.header.opcode),
        " (",
        init.header.opcode,
        ")"));
  }

  fuse_init_out connInfo = {};
  connInfo.major = init.init.major;
  connInfo.minor = init.init.minor;
  connInfo.max_write = bufferSize_ - 4096;

  connInfo.max_readahead = init.init.max_readahead;

  const auto capable = init.init.flags;
  auto& want = connInfo.flags;

  // TODO: follow up and look at the new flags; particularly
  // FUSE_DO_READDIRPLUS, FUSE_READDIRPLUS_AUTO. FUSE_SPLICE_XXX are interesting
  // too, but may not directly benefit eden today.
  //
  // FUSE_ATOMIC_O_TRUNC is a nice optimization when the kernel supports it
  // and the FUSE daemon requires handling open/release for stateful file
  // handles. But FUSE_NO_OPEN_SUPPORT is superior, so edenfs has no need for
  // FUSE_ATOMIC_O_TRUNC. Also, on older kernels, it triggers a kernel bug.
  // See test_mmap_is_null_terminated_after_truncate_and_write_to_overlay
  // in mmap_test.py.

  // We can handle reads concurrently with any other type of request.
  want |= FUSE_ASYNC_READ;
  // We handle writes of any size.
  want |= FUSE_BIG_WRITES;

#ifdef __linux__
  // We're happy to let the kernel cache readlink responses.
  want |= FUSE_CACHE_SYMLINKS;
  // We can handle almost any request in parallel.
  want |= FUSE_PARALLEL_DIROPS;
  // File handles are stateless so the kernel does not need to send open() and
  // release().
  want |= FUSE_NO_OPEN_SUPPORT;
  // File handles are stateless so the kernel does not need to send
  // open() and release().
  want |= FUSE_NO_OPENDIR_SUPPORT;
#endif

  // Only return the capabilities the kernel supports.
  want &= capable;

  XLOG(DBG1) << "Speaking fuse protocol kernel=" << init.init.major << "."
             << init.init.minor << " local=" << FUSE_KERNEL_VERSION << "."
             << FUSE_KERNEL_MINOR_VERSION << " on mount \"" << mountPath_
             << "\", max_write=" << connInfo.max_write
             << ", max_readahead=" << connInfo.max_readahead
             << ", capable=" << flagsToLabel(capsLabels, capable)
             << ", want=" << flagsToLabel(capsLabels, want);

  if (init.init.major != FUSE_KERNEL_VERSION) {
    replyError(init.header, EPROTO);
    throw std::runtime_error(folly::to<std::string>(
        "Unsupported FUSE kernel version ",
        init.init.major,
        ".",
        init.init.minor,
        " while initializing \"",
        mountPath_,
        "\""));
  }

  // Update connInfo_
  // We have not started the other worker threads yet, so this is safe
  // to update without synchronization.
  connInfo_ = connInfo;

  // Send the INIT reply before informing the Dispatcher or signalling
  // initPromise_, so that the kernel will put the mount point in use and will
  // not block further filesystem access on us while running the Dispatcher
  // callback code.
#ifdef __linux__
  static_assert(
      FUSE_KERNEL_MINOR_VERSION > 22,
      "Your kernel headers are too old to build Eden.");
  if (init.init.minor > 22) {
    sendReply(init.header, connInfo);
  } else {
    // If the protocol version predates the expansion of fuse_init_out, only
    // send the start of the packet.
    static_assert(FUSE_COMPAT_22_INIT_OUT_SIZE <= sizeof(connInfo));
    sendReply(
        init.header,
        ByteRange{reinterpret_cast<const uint8_t*>(&connInfo),
                  FUSE_COMPAT_22_INIT_OUT_SIZE});
  }
#elif defined(__APPLE__)
  static_assert(
      FUSE_KERNEL_MINOR_VERSION == 19,
      "osxfuse: API/ABI likely changed, may need something like the"
      " linux code above to send the correct response to the kernel");
  sendReply(init.header, connInfo);
#endif

  dispatcher_->initConnection(connInfo);
}

void FuseChannel::processSession() {
  std::vector<char> buf(bufferSize_);
  // Save this for the sanity check later in the loop to avoid
  // additional syscalls on each loop iteration.
  auto myPid = getpid();

  while (!stop_.load(std::memory_order_relaxed)) {
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(fuseDevice_.fd(), buf.data(), buf.size());
    if (res < 0) {
      int error = errno;
      if (stop_.load(std::memory_order_relaxed)) {
        break;
      }

      if (error == EINTR || error == EAGAIN) {
        // If we got interrupted by a signal while reading the next
        // fuse command, we will simply retry and read the next thing.
        continue;
      } else if (error == ENOENT) {
        // According to comments in the libfuse code:
        // ENOENT means the operation was interrupted; it's safe to restart
        continue;
      } else if (error == ENODEV) {
        // ENODEV means the filesystem was unmounted
        folly::call_once(unmountLogFlag_, [this] {
          XLOG(DBG3) << "received unmount event ENODEV on mount " << mountPath_;
        });
        requestSessionExit(StopReason::UNMOUNTED);
        break;
      } else {
        XLOG(WARNING) << "error reading from fuse channel: "
                      << folly::errnoStr(error);
        requestSessionExit(StopReason::FUSE_READ_ERROR);
        break;
      }
    }

    const auto arg_size = static_cast<size_t>(res);
    if (arg_size < sizeof(struct fuse_in_header)) {
      if (arg_size == 0) {
        // This code path is hit when a fake FUSE channel is closed in our unit
        // tests.  On real FUSE channels we should get ENODEV to indicate that
        // the FUSE channel was shut down.  However, in our unit tests that use
        // fake FUSE connections we cannot send an ENODEV error, and so we just
        // close the channel instead.
        requestSessionExit(StopReason::UNMOUNTED);
      } else {
        // We got a partial FUSE header.  This shouldn't ever happen unless
        // there is a bug in the FUSE kernel code.
        XLOG(ERR) << "read truncated message from kernel fuse device: len="
                  << arg_size;
        requestSessionExit(StopReason::FUSE_TRUNCATED_REQUEST);
      }
      return;
    }

    const auto* header = reinterpret_cast<fuse_in_header*>(buf.data());
    const uint8_t* arg = reinterpret_cast<const uint8_t*>(header + 1);

    XLOG(DBG7) << "fuse request opcode=" << header->opcode << " "
               << fuseOpcodeName(header->opcode) << " unique=" << header->unique
               << " len=" << header->len << " nodeid=" << header->nodeid
               << " uid=" << header->uid << " gid=" << header->gid
               << " pid=" << header->pid;

    // Sanity check to ensure that the request wasn't from ourself.
    //
    // We should never make requests to ourself via normal filesytem
    // operations going through the kernel.  Otherwise we risk deadlocks if the
    // kernel calls us while holding an inode lock, and we then end up making a
    // filesystem call that need the same inode lock.  We will then not be able
    // to resolve this deadlock on kernel inode locks without rebooting the
    // system.
    if (UNLIKELY(static_cast<pid_t>(header->pid) == myPid)) {
      replyError(*header, EIO);
      XLOG(CRITICAL) << "Received FUSE request from our own pid: opcode="
                     << header->opcode << " nodeid=" << header->nodeid
                     << " pid=" << header->pid;
      continue;
    }

    ProcessAccessLog::AccessType type;
    if (isReadOperation(header->opcode)) {
      type = ProcessAccessLog::AccessType::FuseRead;
    } else if (isWriteOperation(header->opcode)) {
      type = ProcessAccessLog::AccessType::FuseWrite;
    } else {
      type = ProcessAccessLog::AccessType::FuseOther;
    }
    processAccessLog_.recordAccess(header->pid, type);

    switch (header->opcode) {
      case FUSE_INIT:
        replyError(*header, EPROTO);
        throw std::runtime_error(
            "received FUSE_INIT after we have been initialized!?");

      case FUSE_GETLK:
      case FUSE_SETLK:
      case FUSE_SETLKW:
        // Deliberately not handling locking; this causes
        // the kernel to do it for us
        XLOG(DBG7) << fuseOpcodeName(header->opcode);
        replyError(*header, ENOSYS);
        break;

#ifdef __linux__
      case FUSE_LSEEK:
        // We only support stateless file handles, so lseek() is meaningless
        // for us.  Returning ENOSYS causes the kernel to implement it for us,
        // and will cause it to stop sending subsequent FUSE_LSEEK requests.
        XLOG(DBG7) << "FUSE_LSEEK";
        replyError(*header, ENOSYS);
        break;
#endif

      case FUSE_POLL:
        // We do not currently implement FUSE_POLL.
        XLOG(DBG7) << "FUSE_POLL";
        replyError(*header, ENOSYS);
        break;

      case FUSE_INTERRUPT: {
        // no reply is required
        XLOG(DBG7) << "FUSE_INTERRUPT";
        // Ignore it: we don't have a reliable way to guarantee
        // that interrupting functions correctly.
        // In addition, the kernel (certainly on macOS) may recycle
        // ids too quickly for us to safely track by `unique` id.
        break;
      }

      case FUSE_DESTROY:
        XLOG(DBG7) << "FUSE_DESTROY";
        dispatcher_->destroy();
        // FUSE on linux doesn't care whether we reply to FUSE_DESTROY
        // but the macOS implementation blocks the unmount syscall until
        // we have responded, which in turn blocks our attempt to gracefully
        // unmount, so we respond here.  It doesn't hurt Linux to respond
        // so we do it for both platforms.
        replyError(*header, 0);
        break;

      case FUSE_NOTIFY_REPLY:
        XLOG(DBG7) << "FUSE_NOTIFY_REPLY";
        // Don't strictly need to do anything here, but may want to
        // turn the kernel notifications in Futures and use this as
        // a way to fulfil the promise
        break;

      case FUSE_IOCTL:
        // Rather than the default ENOSYS, we need to return ENOTTY
        // to indicate that the requested ioctl is not supported
        replyError(*header, ENOTTY);
        break;

      default: {
        const auto handlerIter = handlerMap_.find(header->opcode);
        if (handlerIter != handlerMap_.end()) {
          // Start a new request and associate it with the current thread.
          // It will be disassociated when we leave this scope, but will
          // propagate across any futures that are spawned as part of this
          // request.
          RequestContextScopeGuard requestContextGuard;

          auto& request = RequestData::create(this, *header, dispatcher_);
          uint64_t requestId;
          {
            // Save a weak reference to this new request context.
            // We use this to enable getOutstandingRequests() for debugging
            // purposes, as well as to determine when all requests are done.
            // We allocate our own request Id for this purpose, as the
            // kernel may recycle `unique` values more quickly than the
            // lifecycle of our state here.
            auto state = state_.wlock();
            requestId = state->nextRequestId++;
            state->requests.emplace(
                requestId,
                std::weak_ptr<folly::RequestContext>(
                    RequestContext::saveContext()));
          }
          const auto& entry = handlerIter->second;

          request
              .catchErrors(folly::makeFutureWith([&] {
                             request.startRequest(
                                 dispatcher_->getStats(), entry.histogram);
                             return (this->*entry.handler)(
                                 &request.getReq(), arg);
                           })
                               .within(requestTimeout_))
              .ensure([this, requestId] {
                auto state = state_.wlock();

                // Remove the request from the map
                state->requests.erase(requestId);

                // We may be complete; check to see if all requests are
                // done and whether there are any threads remaining.
                if (state->requests.empty() &&
                    state->stoppedThreads == numThreads_) {
                  sessionComplete(std::move(state));
                }
              });
          break;
        }

        const auto opcode = header->opcode;
        tryRlockCheckBeforeUpdate<folly::Unit>(
            unhandledOpcodes_,
            [&](const auto& unhandledOpcodes) -> std::optional<folly::Unit> {
              if (unhandledOpcodes.find(opcode) != unhandledOpcodes.end()) {
                return folly::unit;
              }
              return std::nullopt;
            },
            [&](auto& unhandledOpcodes) -> folly::Unit {
              XLOG(WARN) << "unhandled fuse opcode " << opcode << "("
                         << fuseOpcodeName(opcode) << ")";
              unhandledOpcodes->insert(opcode);
              return folly::unit;
            });

        try {
          replyError(*header, ENOSYS);
        } catch (const std::system_error& exc) {
          XLOG(ERR) << "Failed to write error response to fuse: " << exc.what();
          requestSessionExit(StopReason::FUSE_WRITE_ERROR);
          return;
        }
        break;
      }
    }
  }
}

bool FuseChannel::isReadOperation(FuseOpcode op) {
  static constexpr FuseOpcode READ_OPS[] = {
      FUSE_GETATTR,
      FUSE_GETXATTR,
      FUSE_LOOKUP,
      FUSE_READ,
      FUSE_READDIR,
      FUSE_READLINK,
      FUSE_STATFS,
      FUSE_OPENDIR,
      FUSE_RELEASEDIR,
      FUSE_LISTXATTR,
#ifdef __linux__
      FUSE_READDIRPLUS,
#endif
#ifdef __APPLE__
      FUSE_GETXTIMES,
#endif
  };

  auto it = std::find(std::begin(READ_OPS), std::end(READ_OPS), op);
  return it != std::end(READ_OPS);
}

bool FuseChannel::isWriteOperation(FuseOpcode op) {
  static constexpr FuseOpcode WRITE_OPS[] = {
      FUSE_CREATE,
      FUSE_MKDIR,
      FUSE_RENAME,
      FUSE_RMDIR,
      FUSE_SETATTR,
      FUSE_SETXATTR,
      FUSE_UNLINK,
      FUSE_WRITE,
      FUSE_FSYNCDIR,
      FUSE_FSYNC,
      FUSE_SYMLINK,
      FUSE_MKNOD,
      FUSE_LINK,
      FUSE_REMOVEXATTR,
      FUSE_FALLOCATE,
#ifdef __linux__
      FUSE_RENAME2,
#endif
  };

  auto it = std::find(std::begin(WRITE_OPS), std::end(WRITE_OPS), op);
  return it != std::end(WRITE_OPS);
}

void FuseChannel::sessionComplete(folly::Synchronized<State>::LockedPtr state) {
  // Check to see if we should delete ourself after fulfilling
  // sessionCompletePromise_
  bool destroy = state->destroyPending;

  // Build the StopData to return
  StopData data;
  data.reason = state->stopReason;
  if (isFuseDeviceValid(data.reason) && connInfo_.has_value()) {
    data.fuseDevice = std::move(fuseDevice_);
    data.fuseSettings = connInfo_.value();
  }

  // Unlock the state before the remaining steps
  state.unlock();

  // Stop the invalidation thread.  We do not do this when requestSessionExit()
  // is called since we want to continue to allow invalidation requests to be
  // processed until all outstanding requests complete.
  stopInvalidationThread();

  // Fulfill sessionCompletePromise
  sessionCompletePromise_.setValue(std::move(data));

  // Destroy ourself if desired
  if (destroy) {
    delete this;
  }
}

folly::Future<folly::Unit> FuseChannel::fuseRead(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto read = reinterpret_cast<const fuse_read_in*>(arg);

  XLOG(DBG7) << "FUSE_READ";

  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->read(ino, read->size, read->offset)
      .thenValue(
          [](BufVec&& buf) { RequestData::get().sendReply(buf.getIov()); });
}

folly::Future<folly::Unit> FuseChannel::fuseWrite(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto write = reinterpret_cast<const fuse_write_in*>(arg);
  auto bufPtr = reinterpret_cast<const char*>(write + 1);
  if (connInfo_->minor < 9) {
    bufPtr = reinterpret_cast<const char*>(arg) + FUSE_COMPAT_WRITE_IN_SIZE;
  }
  XLOG(DBG7) << "FUSE_WRITE " << write->size << " @" << write->offset;

  auto ino = InodeNumber{header->nodeid};
  return dispatcher_
      ->write(ino, folly::StringPiece{bufPtr, write->size}, write->offset)
      .thenValue([](size_t written) {
        fuse_write_out out = {};
        out.size = written;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseLookup(
    const fuse_in_header* header,
    const uint8_t* arg) {
  PathComponentPiece name{reinterpret_cast<const char*>(arg)};
  const auto parent = InodeNumber{header->nodeid};

  XLOG(DBG7) << "FUSE_LOOKUP parent=" << parent << " name=" << name;

  return dispatcher_->lookup(parent, name).thenValue([](fuse_entry_out param) {
    RequestData::get().sendReply(param);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseForget(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto forget = reinterpret_cast<const fuse_forget_in*>(arg);
  XLOG(DBG7) << "FUSE_FORGET inode=" << header->nodeid
             << " nlookup=" << forget->nlookup;
  dispatcher_->forget(InodeNumber{header->nodeid}, forget->nlookup);
  RequestData::get().replyNone();
  return folly::unit;
}

folly::Future<folly::Unit> FuseChannel::fuseGetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_GETATTR inode=" << header->nodeid;
  return dispatcher_->getattr(InodeNumber{header->nodeid})
      .thenValue([](Dispatcher::Attr attr) {
        RequestData::get().sendReply(attr.asFuseAttr());
      });
}

folly::Future<folly::Unit> FuseChannel::fuseSetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto setattr = reinterpret_cast<const fuse_setattr_in*>(arg);
  XLOG(DBG7) << "FUSE_SETATTR inode=" << header->nodeid;
  return dispatcher_->setattr(InodeNumber{header->nodeid}, *setattr)
      .thenValue([](Dispatcher::Attr attr) {
        RequestData::get().sendReply(attr.asFuseAttr());
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReadLink(
    const fuse_in_header* header,
    const uint8_t* /*arg*/) {
  XLOG(DBG7) << "FUSE_READLINK inode=" << header->nodeid;
  return dispatcher_
      ->readlink(
          InodeNumber{header->nodeid},
  /*kernelCachesReadlink=*/
#ifdef FUSE_CACHE_SYMLINKS
          connInfo_->flags & FUSE_CACHE_SYMLINKS
#else
          false
#endif
          )
      .thenValue([](std::string&& str) {
        RequestData::get().sendReply(folly::StringPiece(str));
      });
}

folly::Future<folly::Unit> FuseChannel::fuseSymlink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  XLOG(DBG7) << "FUSE_SYMLINK";
  const PathComponentPiece name{nameStr};
  const StringPiece link{nameStr + name.stringPiece().size() + 1};

  return dispatcher_->symlink(InodeNumber{header->nodeid}, name, link)
      .thenValue(
          [](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseMknod(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nod = reinterpret_cast<const fuse_mknod_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(nod + 1);

  if (connInfo_->minor >= 12) {
    // Kernel passes umask in fuse_mknod_in, but unless FUSE_CAP_DONT_MASK is
    // set, the kernel has already masked it out in mode.
    // https://sourceforge.net/p/fuse/mailman/message/22844100/
  } else {
    // Else: no umask or padding fields available
    nameStr = reinterpret_cast<const char*>(arg) + FUSE_COMPAT_MKNOD_IN_SIZE;
  }

  const PathComponentPiece name{nameStr};
  XLOG(DBG7) << "FUSE_MKNOD " << name;

  return dispatcher_
      ->mknod(InodeNumber{header->nodeid}, name, nod->mode, nod->rdev)
      .thenValue(
          [](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseMkdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto dir = reinterpret_cast<const fuse_mkdir_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(dir + 1);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_MKDIR " << name;

  // Kernel passes umask in fuse_mkdir_in, but unless FUSE_CAP_DONT_MASK is
  // set, the kernel has already masked it out in mode.
  // https://sourceforge.net/p/fuse/mailman/message/22844100/

  XLOG(DBG7) << "mode = " << dir->mode << "; umask = " << dir->umask;

  return dispatcher_
      ->mkdir(InodeNumber{header->nodeid}, name, dir->mode & ~dir->umask)
      .thenValue(
          [](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseUnlink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_UNLINK " << name;

  return dispatcher_->unlink(InodeNumber{header->nodeid}, name)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseRmdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_RMDIR " << name;

  return dispatcher_->rmdir(InodeNumber{header->nodeid}, name)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseRename(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto rename = reinterpret_cast<const fuse_rename_in*>(arg);
  const auto oldNameStr = reinterpret_cast<const char*>(rename + 1);
  const PathComponentPiece oldName{oldNameStr};
  const PathComponentPiece newName{oldNameStr + oldName.stringPiece().size() +
                                   1};

  XLOG(DBG7) << "FUSE_RENAME " << oldName << " -> " << newName;
  return dispatcher_
      ->rename(
          InodeNumber{header->nodeid},
          oldName,
          InodeNumber{rename->newdir},
          newName)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseLink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto link = reinterpret_cast<const fuse_link_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(link + 1);
  const PathComponentPiece newName{nameStr};

  XLOG(DBG7) << "FUSE_LINK " << newName;

  return dispatcher_
      ->link(InodeNumber{link->oldnodeid}, InodeNumber{header->nodeid}, newName)
      .thenValue(
          [](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseOpen(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPEN";
  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->open(ino, open->flags).thenValue([](uint64_t fh) {
    fuse_open_out out = {};
    out.open_flags |= FOPEN_KEEP_CACHE;
    out.fh = fh;
    RequestData::get().sendReply(out);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseStatFs(
    const fuse_in_header* header,
    const uint8_t* /*arg*/) {
  XLOG(DBG7) << "FUSE_STATFS";
  return dispatcher_->statfs(InodeNumber{header->nodeid})
      .thenValue([](struct fuse_kstatfs&& info) {
        fuse_statfs_out out = {};
        out.st = info;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseRelease(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_RELEASE";
  auto ino = InodeNumber{header->nodeid};
  auto release = reinterpret_cast<const fuse_release_in*>(arg);
  return dispatcher_->release(ino, release->fh).thenValue([](folly::Unit) {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseFsync(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNC";

  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->fsync(ino, datasync).thenValue([](auto&&) {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseSetXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto setxattr = reinterpret_cast<const fuse_setxattr_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(setxattr + 1);
  const StringPiece attrName{nameStr};
  const auto bufPtr = nameStr + attrName.size() + 1;
  const StringPiece value(bufPtr, setxattr->size);

  XLOG(DBG7) << "FUSE_SETXATTR";

  return dispatcher_
      ->setxattr(InodeNumber{header->nodeid}, attrName, value, setxattr->flags)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseGetXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto getxattr = reinterpret_cast<const fuse_getxattr_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(getxattr + 1);
  const StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_GETXATTR";
  return dispatcher_->getxattr(InodeNumber{header->nodeid}, attrName)
      .thenValue([size = getxattr->size](std::string attr) {
        auto& request = RequestData::get();
        if (size == 0) {
          fuse_getxattr_out out = {};
          out.size = attr.size();
          request.sendReply(out);
        } else if (size < attr.size()) {
          request.replyError(ERANGE);
        } else {
          request.sendReply(StringPiece(attr));
        }
      });
}

folly::Future<folly::Unit> FuseChannel::fuseListXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto listattr = reinterpret_cast<const fuse_getxattr_in*>(arg);
  XLOG(DBG7) << "FUSE_LISTXATTR";
  return dispatcher_->listxattr(InodeNumber{header->nodeid})
      .thenValue([size = listattr->size](std::vector<std::string> attrs) {
        auto& request = RequestData::get();

        // Initialize count to include the \0 for each
        // entry.
        size_t count = attrs.size();
        for (const auto& attr : attrs) {
          count += attr.size();
        }

        if (size == 0) {
          // caller is asking for the overall size
          fuse_getxattr_out out = {};
          out.size = count;
          request.sendReply(out);
        } else if (size < count) {
          XLOG(DBG7) << "LISTXATTR input size is " << size << " and count is "
                     << count;
          request.replyError(ERANGE);
        } else {
          std::string buf;
          buf.reserve(count);
          for (const auto& attr : attrs) {
            buf.append(attr);
            buf.push_back(0);
          }
          XLOG(DBG7) << "LISTXATTR: " << buf;
          request.sendReply(folly::StringPiece(buf));
        }
      });
}

folly::Future<folly::Unit> FuseChannel::fuseRemoveXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  const StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_REMOVEXATTR";
  return dispatcher_->removexattr(InodeNumber{header->nodeid}, attrName)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseFlush(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto flush = reinterpret_cast<const fuse_flush_in*>(arg);
  XLOG(DBG7) << "FUSE_FLUSH";

  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->flush(ino, flush->lock_owner).thenValue([](auto&&) {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseOpenDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPENDIR";
  auto ino = InodeNumber{header->nodeid};
  auto minorVersion = connInfo_->minor;
  return dispatcher_->opendir(ino, open->flags)
      .thenValue([minorVersion](uint64_t fh) {
        fuse_open_out out = {};
#ifdef FOPEN_CACHE_DIR
        if (minorVersion >= 28) {
          // Opt into readdir caching.
          out.open_flags |= FOPEN_KEEP_CACHE | FOPEN_CACHE_DIR;
        }
#endif
        out.fh = fh;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReadDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto read = reinterpret_cast<const fuse_read_in*>(arg);
  XLOG(DBG7) << "FUSE_READDIR";
  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->readdir(ino, DirList{read->size}, read->offset, read->fh)
      .thenValue([](DirList&& list) {
        const auto buf = list.getBuf();
        RequestData::get().sendReply(StringPiece{buf});
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReleaseDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_RELEASEDIR";
  auto ino = InodeNumber{header->nodeid};
  auto release = reinterpret_cast<const fuse_release_in*>(arg);
  return dispatcher_->releasedir(ino, release->fh).thenValue([](folly::Unit) {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseFsyncDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNCDIR";

  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->fsyncdir(ino, datasync).thenValue([](auto&&) {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseAccess(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto access = reinterpret_cast<const fuse_access_in*>(arg);
  XLOG(DBG7) << "FUSE_ACCESS";
  return dispatcher_->access(InodeNumber{header->nodeid}, access->mask)
      .thenValue([](auto&&) { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseCreate(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto create = reinterpret_cast<const fuse_create_in*>(arg);
  const PathComponentPiece name{reinterpret_cast<const char*>(create + 1)};
  XLOG(DBG7) << "FUSE_CREATE " << name;
  auto ino = InodeNumber{header->nodeid};
  return dispatcher_->create(ino, name, create->mode, create->flags)
      .thenValue([](fuse_entry_out entry) {
        fuse_open_out out = {};
        out.open_flags |= FOPEN_KEEP_CACHE;

        XLOG(DBG7) << "CREATE fh=" << out.fh << " flags=" << out.open_flags;

        folly::fbvector<iovec> vec;

        // 3 to avoid realloc when sendReply prepends a header to the iovec
        vec.reserve(3);
        vec.push_back(make_iovec(entry));
        vec.push_back(make_iovec(out));

        RequestData::get().sendReply(std::move(vec));
      });
}

folly::Future<folly::Unit> FuseChannel::fuseBmap(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto bmap = reinterpret_cast<const fuse_bmap_in*>(arg);
  XLOG(DBG7) << "FUSE_BMAP";
  return dispatcher_
      ->bmap(InodeNumber{header->nodeid}, bmap->blocksize, bmap->block)
      .thenValue([](uint64_t resultIdx) {
        fuse_bmap_out out;
        out.block = resultIdx;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseBatchForget(
    const fuse_in_header* /*header*/,
    const uint8_t* arg) {
  const auto forgets = reinterpret_cast<const fuse_batch_forget_in*>(arg);
  auto item = reinterpret_cast<const fuse_forget_one*>(forgets + 1);
  const auto end = item + forgets->count;
  XLOG(DBG7) << "FUSE_BATCH_FORGET";

  while (item != end) {
    dispatcher_->forget(InodeNumber{item->nodeid}, item->nlookup);
    ++item;
  }
  return folly::unit;
}

FuseDeviceUnmountedDuringInitialization::
    FuseDeviceUnmountedDuringInitialization(AbsolutePathPiece mountPath)
    : std::runtime_error{folly::to<string>(
          "FUSE mount \"",
          mountPath,
          "\" was unmounted before we received the INIT packet"_sp)} {}

} // namespace eden
} // namespace facebook
