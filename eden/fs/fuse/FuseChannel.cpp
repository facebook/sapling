/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/FuseChannel.h"

#include <folly/experimental/logging/xlog.h>
#include <folly/futures/helpers.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/Request.h>
#include <folly/system/ThreadName.h>
#include <signal.h>
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"

using namespace folly;
using std::string;

namespace facebook {
namespace eden {
namespace fusell {

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
    case FUSE_READDIRPLUS:
      return "FUSE_READDIRPLUS";
    case FUSE_RENAME2:
      return "FUSE_RENAME2";
    case FUSE_LSEEK:
      return "FUSE_LSEEK";

    case CUSE_INIT:
      return "CUSE_INIT";
  }
  return "<unknown>";
}

using Handler = folly::Future<folly::Unit> (
    FuseChannel::*)(const fuse_in_header* header, const uint8_t* arg);

} // namespace

struct FuseChannel::HandlerEntry {
  Handler handler;
  EdenStats::HistogramPtr histogram;
};

const FuseChannel::HandlerMap FuseChannel::handlerMap = {
    {FUSE_READ, {&FuseChannel::fuseRead, &EdenStats::read}},
    {FUSE_WRITE, {&FuseChannel::fuseWrite, &EdenStats::write}},
    {FUSE_LOOKUP, {&FuseChannel::fuseLookup, &EdenStats::lookup}},
    {FUSE_FORGET, {&FuseChannel::fuseForget, &EdenStats::forget}},
    {FUSE_GETATTR, {&FuseChannel::fuseGetAttr, &EdenStats::getattr}},
    {FUSE_SETATTR, {&FuseChannel::fuseSetAttr, &EdenStats::setattr}},
    {FUSE_READLINK, {&FuseChannel::fuseReadLink, &EdenStats::readlink}},
    {FUSE_SYMLINK, {&FuseChannel::fuseSymlink, &EdenStats::symlink}},
    {FUSE_MKNOD, {&FuseChannel::fuseMknod, &EdenStats::mknod}},
    {FUSE_MKDIR, {&FuseChannel::fuseMkdir, &EdenStats::mkdir}},
    {FUSE_UNLINK, {&FuseChannel::fuseUnlink, &EdenStats::unlink}},
    {FUSE_RMDIR, {&FuseChannel::fuseRmdir, &EdenStats::rmdir}},
    {FUSE_RENAME, {&FuseChannel::fuseRename, &EdenStats::rename}},
    {FUSE_LINK, {&FuseChannel::fuseLink, &EdenStats::link}},
    {FUSE_OPEN, {&FuseChannel::fuseOpen, &EdenStats::open}},
    {FUSE_STATFS, {&FuseChannel::fuseStatFs, &EdenStats::statfs}},
    {FUSE_RELEASE, {&FuseChannel::fuseRelease, &EdenStats::release}},
    {FUSE_FSYNC, {&FuseChannel::fuseFsync, &EdenStats::fsync}},
    {FUSE_SETXATTR, {&FuseChannel::fuseSetXAttr, &EdenStats::setxattr}},
    {FUSE_GETXATTR, {&FuseChannel::fuseGetXAttr, &EdenStats::getxattr}},
    {FUSE_LISTXATTR, {&FuseChannel::fuseListXAttr, &EdenStats::listxattr}},
    {FUSE_REMOVEXATTR,
     {&FuseChannel::fuseRemoveXAttr, &EdenStats::removexattr}},
    {FUSE_FLUSH, {&FuseChannel::fuseFlush, &EdenStats::flush}},
    {FUSE_OPENDIR, {&FuseChannel::fuseOpenDir, &EdenStats::opendir}},
    {FUSE_READDIR, {&FuseChannel::fuseReadDir, &EdenStats::readdir}},
    {FUSE_RELEASEDIR, {&FuseChannel::fuseReleaseDir, &EdenStats::releasedir}},
    {FUSE_FSYNCDIR, {&FuseChannel::fuseFsyncDir, &EdenStats::fsyncdir}},
    {FUSE_ACCESS, {&FuseChannel::fuseAccess, &EdenStats::access}},
    {FUSE_CREATE, {&FuseChannel::fuseCreate, &EdenStats::create}},
    {FUSE_BMAP, {&FuseChannel::fuseBmap, &EdenStats::bmap}},
    {FUSE_BATCH_FORGET,
     {&FuseChannel::fuseBatchForget, &EdenStats::forgetmulti}},
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
    if ((flags & it.first) == it.first) {
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
    {FUSE_SPLICE_WRITE, "SPLICE_WRITE"},
    {FUSE_SPLICE_MOVE, "SPLICE_MOVE"},
    {FUSE_SPLICE_READ, "SPLICE_READ"},
    {FUSE_FLOCK_LOCKS, "FLOCK_LOCKS"},
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
#ifdef __APPLE__
    {FUSE_ALLOCATE, "ALLOCATE"},
    {FUSE_EXCHANGE_DATA, "EXCHANGE_DATA"},
    {FUSE_CASE_INSENSITIVE, "CASE_INSENSITIVE"},
    {FUSE_VOL_RENAME, "VOL_RENAME"},
    {FUSE_XTIMES, "XTIMES"},
#endif
};

void FuseChannel::replyError(const fuse_in_header& request, int errorCode) {
  fuse_out_header err;
  err.len = sizeof(err);
  err.error = -errorCode;
  err.unique = request.unique;
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
    } else if (
        runState_.load(std::memory_order_relaxed) != StopReason::RUNNING) {
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
    folly::EventBase* eventBase,
    size_t numThreads,
    Dispatcher* const dispatcher)
    : bufferSize_(std::max(size_t(getpagesize()) + 0x1000, MIN_BUFSIZE)),
      numThreads_(numThreads),
      dispatcher_(dispatcher),
      eventBase_(eventBase),
      mountPath_(mountPath),
      fuseDevice_(std::move(fuseDevice)) {
  CHECK_GE(numThreads_, 1);
}

folly::Future<folly::Unit> FuseChannel::initialize() {
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

void FuseChannel::initializeFromTakeover(fuse_init_out connInfo) {
  connInfo_ = connInfo;
  XLOG(INFO) << "Takeover using max_write=" << connInfo_->max_write
             << ", max_readahead=" << connInfo_->max_readahead
             << ", want=" << flagsToLabel(capsLabels, connInfo_->flags);
  startWorkerThreads();
}

void FuseChannel::startWorkerThreads() {
  auto state = state_.wlock();
  try {
    state->workerThreads.reserve(numThreads_);
    while (state->workerThreads.size() < numThreads_) {
      state->workerThreads.emplace_back([this] { fuseWorkerThread(); });
    }
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Error starting FUSE worker threads: " << exceptionStr(ex);
    // Request any threads we did start to stop now.
    requestSessionExit(state, StopReason::INIT_FAILED);
    throw;
  }
}

FuseChannel::~FuseChannel() {
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
}

FuseChannelData FuseChannel::stealFuseDevice() {
  FuseChannelData data;

  data.connInfo = connInfo_.value();
  // Claim the fd
  std::swap(data.fd, fuseDevice_);

  return data;
}

void FuseChannel::invalidateInode(
    fusell::InodeNumber ino,
    off_t off,
    off_t len) {
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
    XLOG(DBG7) << "invalidateInode ino=" << ino << " off=" << off
               << " len=" << len << " OK!";
  } catch (const std::system_error& exc) {
    XLOG(ERR) << "invalidateInode ino=" << ino << " off=" << off
              << " len=" << len << " FAIL: " << exc.what();
    // Ignore ENOENT.  This can happen for inode numbers that we allocated on
    // our own and haven't actually told the kernel about yet.
    if (exc.code().category() == std::system_category() &&
        exc.code().value() != ENOENT) {
      throwSystemErrorExplicit(
          exc.code().value(), "error invalidating FUSE inode ", ino);
    }
  }
}

void FuseChannel::invalidateEntry(
    fusell::InodeNumber parent,
    PathComponentPiece name) {
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
    if (exc.code().category() == std::system_category() &&
        exc.code().value() != ENOENT) {
      throwSystemErrorExplicit(
          exc.code().value(),
          "error invalidating FUSE entry ",
          name,
          " in directory inode ",
          parent);
    }
  }
}

folly::Future<FuseChannel::StopReason> FuseChannel::getSessionCompleteFuture() {
  return sessionCompletePromise_.getFuture();
}

void FuseChannel::requestSessionExit(StopReason reason) {
  requestSessionExit(state_.wlock(), reason);
}

void FuseChannel::requestSessionExit(
    const Synchronized<State>::LockedPtr& state,
    StopReason reason) {
  StopReason expectedReason = StopReason::RUNNING;
  if (runState_.compare_exchange_strong(
          expectedReason,
          reason,
          std::memory_order_relaxed,
          std::memory_order_relaxed)) {
    // Knock our workers out of their blocking read() syscalls
    for (auto& thr : state->workerThreads) {
      if (thr.joinable() && thr.get_id() != std::this_thread::get_id()) {
        pthread_kill(thr.native_handle(), SIGPIPE);
      }
    }
  }
}

void FuseChannel::initWorkerThread() noexcept {
  try {
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
  initPromise_.setValue();

  // Continue to run like a normal FUSE worker thread.
  fuseWorkerThread();
}

void FuseChannel::fuseWorkerThread() noexcept {
  setThreadName(to<std::string>("fuse", mountPath_.basename()));

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

  // We may be complete; check to see if there are any
  // outstanding requests.  Take care to avoid triggering
  // the promise callbacks while we hold a lock.
  bool complete = false;
  {
    auto state = state_.wlock();
    ++state->stoppedThreads;
    if (state->stoppedThreads == numThreads_) {
      // there may be outstanding requests even though we have
      // now shut down all of the fuse device processing threads.
      // If there are none outstanding then we can consider the
      // FUSE requests all complete.  Otherwise, we have logic
      // to detect the completion transition in the finishRequest()
      // method below.
      complete = state->requests.empty();
    }
  }

  if (complete) {
    eventBase_->runInEventBaseThread([this]() {
      sessionCompletePromise_.setValue(
          runState_.load(std::memory_order_relaxed));
    });
  }
}

void FuseChannel::readInitPacket() {
  struct {
    fuse_in_header header;
    fuse_init_in init;
  } init;

  // Loop until we receive the INIT packet, or until we are stopped.
  while (true) {
    if (runState_.load(std::memory_order_relaxed) != StopReason::RUNNING) {
      throw std::runtime_error(folly::to<string>(
          "FuseChannel for \"",
          mountPath_,
          "\" stopped while waiting for INIT packet"));
    }

    auto res = read(fuseDevice_.fd(), &init, sizeof(init));
    if (res < 0) {
      int errnum = errno;
      if (runState_.load(std::memory_order_relaxed) != StopReason::RUNNING) {
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
        throw std::runtime_error(folly::to<string>(
            "FUSE device for \"",
            mountPath_,
            "\" unmounted before we received INIT request"));
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
      throw std::runtime_error(folly::to<string>(
          "FUSE mount \"",
          mountPath_,
          "\" was unmounted before we received the INIT packet"));
    }

    // Error out if the kernel sends less data than we expected.
    // We currently don't error out for now if we receive more data: maybe this
    // could happen for future kernel versions that speak a newer FUSE protocol
    // with extra fields in fuse_init_in?
    if (res < sizeof(init)) {
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
  connInfo.major = FUSE_KERNEL_VERSION;
  connInfo.minor = FUSE_KERNEL_MINOR_VERSION;
  connInfo.max_write = bufferSize_ - 4096;

  connInfo.max_readahead = init.init.max_readahead;

  const auto& capable = init.init.flags;
  auto& want = connInfo.flags;

  // TODO: follow up and look at the new flags; particularly
  // FUSE_PARALLEL_DIROPS, FUSE_DO_READDIRPLUS,
  // FUSE_READDIRPLUS_AUTO. FUSE_SPLICE_XXX are interesting too,
  // but may not directly benefit eden today.
  //
  // It would be great to enable FUSE_ATOMIC_O_TRUNC but it
  // seems to trigger a kernel/FUSE bug.  See
  // test_mmap_is_null_terminated_after_truncate_and_write_to_overlay
  // in mmap_test.py. FUSE_ATOMIC_O_TRUNC |
  want = capable & (FUSE_BIG_WRITES | FUSE_ASYNC_READ);

  XLOG(INFO) << "Speaking fuse protocol kernel=" << init.init.major << "."
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
  sendReply(init.header, connInfo);
  dispatcher_->initConnection(connInfo);
}

void FuseChannel::processSession() {
  std::vector<char> buf(bufferSize_);
  // Save this for the sanity check later in the loop to avoid
  // additional syscalls on each loop iteration.
  auto myPid = getpid();

  while (runState_.load(std::memory_order_relaxed) == StopReason::RUNNING) {
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(fuseDevice_.fd(), buf.data(), buf.size());
    if (res < 0) {
      res = -errno;
    }

    if (runState_.load(std::memory_order_relaxed) != StopReason::RUNNING) {
      break;
    }

    if (res == -EINTR || res == -EAGAIN) {
      // If we got interrupted by a signal while reading the next
      // fuse command, we will simply retry and read the next thing.
      continue;
    }
    if (res == -ENOENT) {
      // According to comments in the libfuse code:
      // ENOENT means the operation was interrupted; it's safe to restart
      continue;
    }
    if (res == -ENODEV) {
      // ENODEV means the filesystem was unmounted
      requestSessionExit(StopReason::UNMOUNTED);
      break;
    }
    if (res < 0) {
      XLOG(WARNING) << "error reading from fuse channel: "
                    << folly::errnoStr(-res);
      requestSessionExit(StopReason::FUSE_READ_ERROR);
      break;
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
        break;
      }
    }

    const auto* header = reinterpret_cast<fuse_in_header*>(buf.data());
    const uint8_t* arg = reinterpret_cast<const uint8_t*>(header + 1);

    XLOG(DBG7) << "fuse request opcode=" << header->opcode
               << " unique=" << header->unique << " len=" << header->len
               << " nodeid=" << header->nodeid << " uid=" << header->uid
               << " gid=" << header->gid << " pid=" << header->pid;

    // Sanity check to ensure that the request wasn't from ourself.
    //
    // We should never make requests to ourself via normal filesytem
    // operations going through the kernel.  Otherwise we risk deadlocks if the
    // kernel calls us while holding an inode lock, and we then end up making a
    // filesystem call that need the same inode lock.  We will then not be able
    // to resolve this deadlock on kernel inode locks without rebooting the
    // system.
    if (header->pid == myPid) {
      XLOG(DFATAL) << "Received FUSE request from our own pid: opcode="
                   << header->opcode << " nodeid=" << header->nodeid
                   << " pid=" << header->pid;
      replyError(*header, EIO);
      continue;
    }

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
        replyError(*header, ENOSYS);
        break;

      case FUSE_INTERRUPT: {
        // no reply is required
        XLOG(DBG7) << "FUSE_INTERRUPT";
        const auto in = reinterpret_cast<const fuse_interrupt_in*>(arg);

        // Look up the fuse request; if we find it and the context
        // is still alive, ctx will be set to it
        std::shared_ptr<folly::RequestContext> ctx;

        {
          const auto state = state_.wlock();
          const auto requestIter = state->requests.find(in->unique);
          if (requestIter != state->requests.end()) {
            ctx = requestIter->second.lock();
          }
        }

        // If we found an existing request, temporarily activate that request
        // context so that we can test whether the request is definitely a fuse
        // request; if so, interrupt it.
        if (ctx) {
          const RequestContextScopeGuard guard(ctx);
          if (RequestData::isFuseRequest()) {
            RequestData::get().interrupt();
          }
        }

        break;
      }

      case FUSE_DESTROY:
        XLOG(DBG7) << "FUSE_DESTROY";
        dispatcher_->destroy();
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
        const auto handlerIter = handlerMap.find(header->opcode);
        if (handlerIter != handlerMap.end()) {
          // Start a new request and associate it with the current thread.
          // It will be disassociated when we leave this scope, but will
          // propagate across any futures that are spawned as part of this
          // request.
          RequestContextScopeGuard requestContextGuard;

          // Save a weak reference to this new request context.
          // We'll need this to process FUSE_INTERRUPT requests.
          state_.wlock()->requests.emplace(
              header->unique,
              std::weak_ptr<folly::RequestContext>(
                  RequestContext::saveContext()));

          auto& request = RequestData::create(this, *header, dispatcher_);
          const auto& entry = handlerIter->second;

          request.setRequestFuture(
              request.startRequest(dispatcher_->getStats(), entry.histogram)
                  .then([=, &request] {
                    return (this->*entry.handler)(&request.getReq(), arg);
                  }));
          break;
        }

        unhandledOpcodes_.withULockPtr([&](auto ulock) {
          const auto opcode = header->opcode;
          if (ulock->find(opcode) == ulock->end()) {
            XLOG(ERR) << "unhandled fuse opcode " << opcode << "("
                      << fuseOpcodeName(opcode) << ")";
            auto wlock = ulock.moveFromUpgradeToWrite();
            wlock->insert(opcode);
          }
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

void FuseChannel::finishRequest(const fuse_in_header& header) {
  // Remove the current request from the map.
  // We may be complete; check to see if all requests are
  // done and whether there are any threads remaining.
  // Take care to avoid triggering the promise callbacks
  // while we hold a lock.
  bool complete = false;
  {
    auto state = state_.wlock();
    const bool erased = state->requests.erase(header.unique) > 0;

    // We only want to fulfil on the transition, so we take
    // care to gate this on whether we actually erased and
    // element in the map above.
    complete = erased && state->requests.empty() &&
        state->stoppedThreads == numThreads_;
  }
  if (complete) {
    sessionCompletePromise_.setValue(runState_.load(std::memory_order_relaxed));
  }
}

folly::Future<folly::Unit> FuseChannel::fuseRead(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto read = reinterpret_cast<const fuse_read_in*>(arg);

  XLOG(DBG7) << "FUSE_READ";

  auto fh = dispatcher_->getFileHandle(read->fh);
  XLOG(DBG7) << "reading " << read->size << "@" << read->offset;
  return fh->read(read->size, read->offset).then([](BufVec&& buf) {
    RequestData::get().sendReply(buf.getIov());
  });
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

  const auto fh = dispatcher_->getFileHandle(write->fh);

  return fh->write(folly::StringPiece(bufPtr, write->size), write->offset)
      .then([](size_t wrote) {
        fuse_write_out out = {};
        out.size = wrote;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseLookup(
    const fuse_in_header* header,
    const uint8_t* arg) {
  PathComponentPiece name{reinterpret_cast<const char*>(arg)};
  const auto parent = fusell::InodeNumber{header->nodeid};

  XLOG(DBG7) << "FUSE_LOOKUP";

  return dispatcher_->lookup(parent, name).then([](fuse_entry_out param) {
    RequestData::get().sendReply(param);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseForget(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto forget = reinterpret_cast<const fuse_forget_in*>(arg);
  XLOG(DBG7) << "FUSE_FORGET";
  return dispatcher_
      ->forget(fusell::InodeNumber{header->nodeid}, forget->nlookup)
      .then([]() { RequestData::get().replyNone(); });
}

folly::Future<folly::Unit> FuseChannel::fuseGetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_GETATTR";

  // If we're new enough, check to see if a file handle was provided
  if (connInfo_->minor >= 9) {
    const auto getattr = reinterpret_cast<const fuse_getattr_in*>(arg);
    if (getattr->getattr_flags & FUSE_GETATTR_FH) {
      return dispatcher_->getGenericFileHandle(getattr->fh)
          ->getattr()
          .then([](Dispatcher::Attr attr) {
            RequestData::get().sendReply(attr.asFuseAttr());
          });
    }
    // otherwise, fall through to regular inode based lookup
  }

  return dispatcher_->getattr(fusell::InodeNumber{header->nodeid})
      .then([](Dispatcher::Attr attr) {
        RequestData::get().sendReply(attr.asFuseAttr());
      });
}

folly::Future<folly::Unit> FuseChannel::fuseSetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto setattr = reinterpret_cast<const fuse_setattr_in*>(arg);
  XLOG(DBG7) << "FUSE_SETATTR";
  if (setattr->valid & FATTR_FH) {
    return dispatcher_->getGenericFileHandle(setattr->fh)
        ->setattr(*setattr)
        .then([](Dispatcher::Attr attr) {
          RequestData::get().sendReply(attr.asFuseAttr());
        });
  } else {
    return dispatcher_->setattr(fusell::InodeNumber{header->nodeid}, *setattr)
        .then([](Dispatcher::Attr attr) {
          RequestData::get().sendReply(attr.asFuseAttr());
        });
  }
}

folly::Future<folly::Unit> FuseChannel::fuseReadLink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_READLINK";
  return dispatcher_->readlink(fusell::InodeNumber{header->nodeid})
      .then([](std::string&& str) {
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

  return dispatcher_->symlink(fusell::InodeNumber{header->nodeid}, name, link)
      .then([](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseMknod(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nod = reinterpret_cast<const fuse_mknod_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(nod + 1);

  if (connInfo_->minor >= 12) {
    // TODO: Implement proper scheme for mkdir to handle umask and permissions.
    // We don't handle umask here or set permissions properly,
    // but even if we did, TreeInode::getAttr() returns a constexpr
    // for mode because we don't store proper mode on mkdir or mknod.
    // Some ideas for fixing this include:
    // - Add mode attribute to TreeInode::Dir
    // - Add a disk map which tracks permissions and timestamps for objects
    // - Cover both the reading (e.g., mkdir) and reading (e.g., stat) paths
  } else {
    // Else: no umask or padding fields available
    nameStr = reinterpret_cast<const char*>(arg) + FUSE_COMPAT_MKNOD_IN_SIZE;
  }

  const PathComponentPiece name{nameStr};
  XLOG(DBG7) << "FUSE_MKNOD " << name;

  return dispatcher_
      ->mknod(fusell::InodeNumber{header->nodeid}, name, nod->mode, nod->rdev)
      .then([](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseMkdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto dir = reinterpret_cast<const fuse_mkdir_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(dir + 1);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_MKDIR " << name;

  // TODO: Please see the TODO in FuseChannel::fuseMknod explaining
  // why we don't properly handle umask and permissions here and for
  // some ideas of how to fix it.

  XLOG(DBG7) << "mode = " << dir->mode << "; umask = " << dir->umask;

  return dispatcher_
      ->mkdir(
          fusell::InodeNumber{header->nodeid}, name, dir->mode & ~dir->umask)
      .then([](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseUnlink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_UNLINK " << name;

  return dispatcher_->unlink(fusell::InodeNumber{header->nodeid}, name)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseRmdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto nameStr = reinterpret_cast<const char*>(arg);
  const PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_RMDIR " << name;

  return dispatcher_->rmdir(fusell::InodeNumber{header->nodeid}, name)
      .then([]() { RequestData::get().replyError(0); });
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
          fusell::InodeNumber{header->nodeid},
          oldName,
          fusell::InodeNumber{rename->newdir},
          newName)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseLink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto link = reinterpret_cast<const fuse_link_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(link + 1);
  const PathComponentPiece newName{nameStr};

  XLOG(DBG7) << "FUSE_LINK " << newName;

  return dispatcher_
      ->link(
          fusell::InodeNumber{link->oldnodeid},
          fusell::InodeNumber{header->nodeid},
          newName)
      .then([](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseOpen(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPEN";
  return dispatcher_->open(fusell::InodeNumber{header->nodeid}, open->flags)
      .then([this](std::shared_ptr<FileHandle> fh) {
        if (!fh) {
          throw std::runtime_error("Dispatcher::open failed to set fh");
        }
        fuse_open_out out = {};
        if (fh->usesDirectIO()) {
          out.open_flags |= FOPEN_DIRECT_IO;
        }
        if (fh->preserveCache()) {
          out.open_flags |= FOPEN_KEEP_CACHE;
        }
        if (!fh->isSeekable()) {
          out.open_flags |= FOPEN_NONSEEKABLE;
        }
        out.fh = dispatcher_->getFileHandles().recordHandle(std::move(fh));
        try {
          RequestData::get().sendReply(out);
        } catch (const std::system_error&) {
          // Was interrupted, tidy up.
          dispatcher_->getFileHandles().forgetGenericHandle(out.fh);
          throw;
        }
      });
}

folly::Future<folly::Unit> FuseChannel::fuseStatFs(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_STATFS";
  return dispatcher_->statfs(fusell::InodeNumber{header->nodeid})
      .then([](struct fuse_kstatfs&& info) {
        fuse_statfs_out out = {};
        out.st = info;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseRelease(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto release = reinterpret_cast<const fuse_release_in*>(arg);
  XLOG(DBG7) << "FUSE_RELEASE";
  dispatcher_->getFileHandles().forgetGenericHandle(release->fh);
  RequestData::get().replyError(0);
  return Unit{};
}

folly::Future<folly::Unit> FuseChannel::fuseFsync(

    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNC";

  auto fh = dispatcher_->getFileHandle(fsync->fh);
  return fh->fsync(datasync).then([]() { RequestData::get().replyError(0); });
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
      ->setxattr(
          fusell::InodeNumber{header->nodeid}, attrName, value, setxattr->flags)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseGetXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto getxattr = reinterpret_cast<const fuse_getxattr_in*>(arg);
  const auto nameStr = reinterpret_cast<const char*>(getxattr + 1);
  const StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_GETXATTR";
  return dispatcher_->getxattr(fusell::InodeNumber{header->nodeid}, attrName)
      .then([size = getxattr->size](std::string attr) {
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
  return dispatcher_->listxattr(fusell::InodeNumber{header->nodeid})
      .then([size = listattr->size](std::vector<std::string> attrs) {
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
          folly::join('\0', attrs, buf);
          buf.push_back('\0');
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
  return dispatcher_->removexattr(fusell::InodeNumber{header->nodeid}, attrName)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseFlush(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto flush = reinterpret_cast<const fuse_flush_in*>(arg);
  XLOG(DBG7) << "FUSE_FLUSH";
  const auto fh = dispatcher_->getFileHandle(flush->fh);

  return fh->flush(flush->lock_owner).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseOpenDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPENDIR";
  return dispatcher_->opendir(fusell::InodeNumber{header->nodeid}, open->flags)
      .then([this](std::shared_ptr<DirHandle> dh) {
        if (!dh) {
          throw std::runtime_error("Dispatcher::opendir failed to set dh");
        }
        fuse_open_out out = {};
        out.fh = dispatcher_->getFileHandles().recordHandle(std::move(dh));
        XLOG(DBG7) << "OPENDIR fh=" << out.fh;
        try {
          RequestData::get().sendReply(out);
        } catch (const std::system_error&) {
          // Was interrupted, tidy up
          dispatcher_->getFileHandles().forgetGenericHandle(out.fh);
          throw;
        }
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReadDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto read = reinterpret_cast<const fuse_read_in*>(arg);
  XLOG(DBG7) << "FUSE_READDIR";
  const auto dh = dispatcher_->getDirHandle(read->fh);
  return dh->readdir(DirList(read->size), read->offset)
      .then([](DirList&& list) {
        const auto buf = list.getBuf();
        RequestData::get().sendReply(StringPiece(buf));
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReleaseDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto release = reinterpret_cast<const fuse_release_in*>(arg);
  XLOG(DBG7) << "FUSE_RELEASEDIR";
  dispatcher_->getFileHandles().forgetGenericHandle(release->fh);
  RequestData::get().replyError(0);
  return Unit{};
}

folly::Future<folly::Unit> FuseChannel::fuseFsyncDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  const bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNCDIR";

  auto dh = dispatcher_->getDirHandle(fsync->fh);
  return dh->fsyncdir(datasync).then(
      []() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseAccess(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto access = reinterpret_cast<const fuse_access_in*>(arg);
  XLOG(DBG7) << "FUSE_ACCESS";
  return dispatcher_->access(fusell::InodeNumber{header->nodeid}, access->mask)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseCreate(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto create = reinterpret_cast<const fuse_create_in*>(arg);
  const PathComponentPiece name{reinterpret_cast<const char*>(create + 1)};
  XLOG(DBG7) << "FUSE_CREATE " << name;
  return dispatcher_
      ->create(
          fusell::InodeNumber{header->nodeid},
          name,
          create->mode,
          create->flags)
      .then([this](Dispatcher::Create info) {
        fuse_open_out out = {};
        if (info.fh->usesDirectIO()) {
          out.open_flags |= FOPEN_DIRECT_IO;
        }
        if (info.fh->preserveCache()) {
          out.open_flags |= FOPEN_KEEP_CACHE;
        }
        if (!info.fh->isSeekable()) {
          out.open_flags |= FOPEN_NONSEEKABLE;
        }
        out.fh = dispatcher_->getFileHandles().recordHandle(std::move(info.fh));

        XLOG(DBG7) << "CREATE fh=" << out.fh << " flags=" << out.open_flags;

        folly::fbvector<iovec> vec;

        // 3 to avoid realloc when sendRepy prepends a header to the iovec
        vec.reserve(3);

        vec.push_back(make_iovec(info.entry));
        vec.push_back(make_iovec(out));

        try {
          RequestData::get().sendReply(std::move(vec));
        } catch (const std::system_error&) {
          // Was interrupted, tidy up.
          dispatcher_->getFileHandles().forgetGenericHandle(out.fh);
          throw;
        }
      });
}

folly::Future<folly::Unit> FuseChannel::fuseBmap(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto bmap = reinterpret_cast<const fuse_bmap_in*>(arg);
  XLOG(DBG7) << "FUSE_BMAP";
  return dispatcher_
      ->bmap(fusell::InodeNumber{header->nodeid}, bmap->blocksize, bmap->block)
      .then([](uint64_t resultIdx) {
        fuse_bmap_out out;
        out.block = resultIdx;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseBatchForget(
    const fuse_in_header* header,
    const uint8_t* arg) {
  const auto forgets = reinterpret_cast<const fuse_batch_forget_in*>(arg);
  auto item = reinterpret_cast<const fuse_forget_one*>(forgets + 1);
  const auto end = item + forgets->count;
  XLOG(DBG7) << "FUSE_BATCH_FORGET";

  while (item != end) {
    dispatcher_->forget(fusell::InodeNumber{item->nodeid}, item->nlookup);
    ++item;
  }
  return Unit{};
}

} // namespace fusell
} // namespace eden
} // namespace facebook
