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
#include <folly/io/async/Request.h>
#include <folly/system/ThreadName.h>
#include <signal.h>
#include <sys/statvfs.h>
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

namespace {

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

struct HandlerEntry {
  Handler handler;
  EdenStats::HistogramPtr histogram;
};

const std::unordered_map<uint32_t, HandlerEntry> handlerMap = {
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
} // namespace

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
  for (auto& it : labels) {
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
  if (res < 0) {
    throwSystemErrorExplicit(errno, "replyError: error writing to fuse device");
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
  auto header = reinterpret_cast<fuse_out_header*>(iov[0].iov_base);
  header->len = 0;
  for (size_t i = 0; i < count; ++i) {
    header->len += iov[i].iov_len;
  }

  auto res = writev(fuseDevice_.fd(), iov, count);
  int err = errno;
  XLOG(DBG7) << "sendRawReply: unique=" << header->unique
             << " header->len=" << header->len << " wrote=" << res;

  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal.  We don't need to log this,
      // but will propagate it back to our caller.
    } else if (sessionFinished_.load()) {
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
    : dispatcher_(dispatcher),
      fuseDevice_(std::move(fuseDevice)),
      eventBase_(eventBase),
      numThreads_(numThreads),
      mountPath_(mountPath) {
  workerThreads_.reserve(numThreads);
  for (auto i = 0; i < numThreads; ++i) {
    workerThreads_.emplace_back(
        std::thread([this, i] { fuseWorkerThread(i); }));
  }
}

FuseChannel::~FuseChannel() {
  CHECK_EQ(joinedThreads_, numThreads_)
      << "we MUST have joined all of the threads";
}

folly::File FuseChannel::stealFuseDevice() {
  // Claim the fd
  folly::File fd;
  std::swap(fd, fuseDevice_);

  return fd;
}

void FuseChannel::invalidateInode(
    fusell::InodeNumber ino,
    off_t off,
    off_t len) {
  fuse_notify_inval_inode_out notify;
  notify.ino = ino;
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

  fuse_notify_inval_entry_out notify;
  memset(&notify, 0, sizeof(notify));
  notify.parent = parent;
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

folly::Future<folly::Unit> FuseChannel::getThreadsFinishedFuture() {
  return threadsFinishedPromise_.getFuture();
}

folly::Future<folly::Unit> FuseChannel::getThreadsStoppingFuture() {
  return threadsStoppingPromise_.getFuture();
}

folly::Future<folly::Unit> FuseChannel::getSessionCompleteFuture() {
  return sessionCompletePromise_.getFuture();
}

void FuseChannel::requestSessionExit() {
  bool no = false;
  if (sessionFinished_.compare_exchange_strong(no, true)) {
    // We transitioned it to stopping.  Let's notify potentially
    // interested folks about this
    threadsStoppingPromise_.setValue();

    // Knock our workers out of their blocking read() syscalls
    for (auto& thr : workerThreads_) {
      if (thr.get_id() != std::this_thread::get_id()) {
        pthread_kill(thr.native_handle(), SIGPIPE);
      }
    }
  }
}

void FuseChannel::fuseWorkerThread(size_t threadNumber) {
  setThreadName(to<std::string>("fuse", mountPath_.basename()));
  processSession();
  eventBase_->runInEventBaseThread([this, threadNumber]() {
    workerThreads_[threadNumber].join();
    if (++joinedThreads_ == numThreads_) {
      threadsFinishedPromise_.setValue();

      // We may be complete; check to see if there are any
      // outstanding requests.  Take care to avoid triggering
      // the promise callbacks while we hold a lock.
      bool complete = false;
      {
        auto requests = requests_.wlock();
        // there may be outstanding requests even though we have
        // now shut down all of the fuse device processing threads.
        // If there are none outstanding then we can consider the
        // FUSE requests all complete.  Otherwise, we have logic
        // to detect the completion transition in the finishRequest()
        // method below.
        complete = requests->size() == 0;
      }
      if (complete) {
        sessionCompletePromise_.setValue();
      }
    }
  });
}

void FuseChannel::processSession() {
  // This is the minimum size used by libfuse so we use it too!
  constexpr size_t MIN_BUFSIZE = 0x21000;
  std::vector<char> buf(std::max(size_t(getpagesize()) + 0x1000, MIN_BUFSIZE));

  while (!sessionFinished_.load()) {
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(fuseDevice_.fd(), buf.data(), buf.size());
    if (res < 0) {
      res = -errno;
    }

    if (sessionFinished_.load()) {
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
      requestSessionExit();
      break;
    }
    if (res < 0) {
      XLOG(WARNING) << "error reading from fuse channel: "
                    << folly::errnoStr(-res);
      requestSessionExit();
      break;
    }

    auto arg_size = static_cast<size_t>(res);

    if (arg_size < sizeof(struct fuse_in_header)) {
      XLOG(ERR) << "read truncated message from kernel fuse device: len="
                << arg_size;
      requestSessionExit();
      break;
    }

    const auto* header = reinterpret_cast<fuse_in_header*>(buf.data());
    const uint8_t* arg = reinterpret_cast<const uint8_t*>(header + 1);

    XLOG(DBG7) << "fuse request opcode=" << header->opcode
               << " unique=" << header->unique << " len=" << header->len
               << " nodeid=" << header->nodeid << " uid=" << header->uid
               << " gid=" << header->gid << " pid=" << header->pid;

    switch (header->opcode) {
      case FUSE_INIT: {
        auto in = reinterpret_cast<const fuse_init_in*>(arg);

        memset(&connInfo_, 0, sizeof(connInfo_));
        connInfo_.major = FUSE_KERNEL_VERSION;
        connInfo_.minor = FUSE_KERNEL_MINOR_VERSION;
        connInfo_.max_write = buf.size() - 4096;

        connInfo_.max_readahead = in->max_readahead;

        const auto& capable = in->flags;
        auto& want = connInfo_.flags;

        want |=
            capable &
            (
                // TODO: follow up and look at the new flags; particularly
                // FUSE_PARALLEL_DIROPS, FUSE_DO_READDIRPLUS,
                // FUSE_READDIRPLUS_AUTO. FUSE_SPLICE_XXX are interesting too,
                // but may not directly benefit eden today.

                // It would be great to enable FUSE_ATOMIC_O_TRUNC but it
                // seems to trigger a kernel/FUSE bug.  See
                // test_mmap_is_null_terminated_after_truncate_and_write_to_overlay
                // in mmap_test.py. FUSE_ATOMIC_O_TRUNC |
                FUSE_BIG_WRITES | FUSE_ASYNC_READ);

        XLOG(INFO) << "Speaking fuse protocol kernel=" << in->major << "."
                   << in->minor << " local=" << FUSE_KERNEL_VERSION << "."
                   << FUSE_KERNEL_MINOR_VERSION
                   << ", max_write=" << connInfo_.max_write
                   << ", max_readahead=" << connInfo_.max_readahead
                   << ", capable=" << flagsToLabel(capsLabels, capable)
                   << ", want=" << flagsToLabel(capsLabels, want);

        if (in->major != FUSE_KERNEL_VERSION) {
          replyError(*header, EPROTO);
          throw std::runtime_error("fuse kernel major version is unsupported");
        }

        sendReply(*header, connInfo_);
        dispatcher_->initConnection(connInfo_);
        break;
      }

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
        auto in = reinterpret_cast<const fuse_interrupt_in*>(arg);

        // Look up the fuse request; if we find it and the context
        // is still alive, ctx will be set to it
        std::shared_ptr<folly::RequestContext> ctx;

        {
          auto requests = requests_.wlock();
          auto requestIter = requests->find(in->unique);
          if (requestIter != requests->end()) {
            ctx = requestIter->second.lock();
          }
        }

        // If we found an existing request, temporarily activate that request
        // context so that we can test whether the request is definitely a fuse
        // request; if so, interrupt it.
        if (ctx) {
          RequestContextScopeGuard guard(ctx);
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
        auto handlerIter = handlerMap.find(header->opcode);
        if (handlerIter != handlerMap.end()) {
          // Start a new request and associate it with the current thread.
          // It will be disassociated when we leave this scope, but will
          // propagate across any futures that are spawned as part of this
          // request.
          RequestContextScopeGuard requestContextGuard;

          // Save a weak reference to this new request context.
          // We'll need this to process FUSE_INTERRUPT requests.
          requests_->emplace(
              header->unique,
              std::weak_ptr<folly::RequestContext>(
                  RequestContext::saveContext()));

          auto& request = RequestData::create(this, *header, dispatcher_);
          auto& entry = handlerIter->second;

          request.setRequestFuture(
              request.startRequest(dispatcher_->getStats(), entry.histogram)
                  .then([=, &request] {
                    return (this->*entry.handler)(&request.getReq(), arg);
                  }));
          break;
        }

        unhandledOpcodes_.withULockPtr([&](auto ulock) {
          auto opcode = header->opcode;
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
          sessionFinished_.store(true);
          requestSessionExit();
        }
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
    auto requests = requests_.wlock();
    bool erased = requests->erase(header.unique) > 0;

    // We only want to fulfil on the transition, so we take
    // care to gate this on whether we actually erased and
    // element in the map above.
    complete = erased && requests->size() == 0 && joinedThreads_ == numThreads_;
  }
  if (complete) {
    sessionCompletePromise_.setValue();
  }
}

folly::Future<folly::Unit> FuseChannel::fuseRead(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto read = reinterpret_cast<const fuse_read_in*>(arg);

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
  auto write = reinterpret_cast<const fuse_write_in*>(arg);
  auto bufPtr = reinterpret_cast<const char*>(write + 1);
  if (connInfo_.minor < 9) {
    bufPtr = reinterpret_cast<const char*>(arg) + FUSE_COMPAT_WRITE_IN_SIZE;
  }
  XLOG(DBG7) << "FUSE_WRITE " << write->size << " @" << write->offset;

  auto fh = dispatcher_->getFileHandle(write->fh);

  return fh->write(folly::StringPiece(bufPtr, write->size), write->offset)
      .then([](size_t wrote) {
        fuse_write_out out;
        memset(&out, 0, sizeof(out));
        out.size = wrote;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseLookup(
    const fuse_in_header* header,
    const uint8_t* arg) {
  PathComponentPiece name{reinterpret_cast<const char*>(arg)};
  auto parent = header->nodeid;

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
  return dispatcher_->forget(header->nodeid, forget->nlookup).then([]() {
    RequestData::get().replyNone();
  });
}

folly::Future<folly::Unit> FuseChannel::fuseGetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_GETATTR";

  // If we're new enough, check to see if a file handle was provided
  if (connInfo_.minor >= 9) {
    auto getattr = reinterpret_cast<const fuse_getattr_in*>(arg);
    if (getattr->getattr_flags & FUSE_GETATTR_FH) {
      return dispatcher_->getGenericFileHandle(getattr->fh)
          ->getattr()
          .then([](Dispatcher::Attr attr) {
            RequestData::get().sendReply(attr.asFuseAttr());
          });
    }
    // otherwise, fall through to regular inode based lookup
  }

  return dispatcher_->getattr(header->nodeid).then([](Dispatcher::Attr attr) {
    RequestData::get().sendReply(attr.asFuseAttr());
  });
}

folly::Future<folly::Unit> FuseChannel::fuseSetAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto setattr = reinterpret_cast<const fuse_setattr_in*>(arg);
  XLOG(DBG7) << "FUSE_SETATTR";
  if (setattr->valid & FATTR_FH) {
    return dispatcher_->getGenericFileHandle(setattr->fh)
        ->setattr(*setattr)
        .then([](Dispatcher::Attr attr) {
          RequestData::get().sendReply(attr.asFuseAttr());
        });
  } else {
    return dispatcher_->setattr(header->nodeid, *setattr)
        .then([](Dispatcher::Attr attr) {
          RequestData::get().sendReply(attr.asFuseAttr());
        });
  }
}

folly::Future<folly::Unit> FuseChannel::fuseReadLink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  XLOG(DBG7) << "FUSE_READLINK";
  return dispatcher_->readlink(header->nodeid).then([](std::string&& str) {
    RequestData::get().sendReply(folly::StringPiece(str));
  });
}

folly::Future<folly::Unit> FuseChannel::fuseSymlink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto nameStr = reinterpret_cast<const char*>(arg);
  XLOG(DBG7) << "FUSE_SYMLINK";
  PathComponentPiece name{nameStr};
  StringPiece link{nameStr + name.stringPiece().size() + 1};

  return dispatcher_->symlink(header->nodeid, name, link)
      .then([](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseMknod(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto nod = reinterpret_cast<const fuse_mknod_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(nod + 1);

  if (connInfo_.minor >= 12) {
    // TODO: do something useful with nod->umask
  } else {
    // Else: no umask or padding fields available
    nameStr = reinterpret_cast<const char*>(arg) + FUSE_COMPAT_MKNOD_IN_SIZE;
  }

  PathComponentPiece name{nameStr};
  XLOG(DBG7) << "FUSE_MKNOD " << name;

  return dispatcher_->mknod(header->nodeid, name, nod->mode, nod->rdev)
      .then([](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseMkdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto dir = reinterpret_cast<const fuse_mkdir_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(dir + 1);
  PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_MKDIR " << name;

  // TODO: do something useful with dir->umask

  return dispatcher_->mkdir(header->nodeid, name, dir->mode)
      .then([](fuse_entry_out entry) { RequestData::get().sendReply(entry); });
}

folly::Future<folly::Unit> FuseChannel::fuseUnlink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto nameStr = reinterpret_cast<const char*>(arg);
  PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_UNLINK " << name;

  return dispatcher_->unlink(header->nodeid, name).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseRmdir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto nameStr = reinterpret_cast<const char*>(arg);
  PathComponentPiece name{nameStr};

  XLOG(DBG7) << "FUSE_RMDIR " << name;

  return dispatcher_->rmdir(header->nodeid, name).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseRename(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto rename = reinterpret_cast<const fuse_rename_in*>(arg);
  auto oldNameStr = reinterpret_cast<const char*>(rename + 1);
  PathComponentPiece oldName{oldNameStr};
  PathComponentPiece newName{oldNameStr + oldName.stringPiece().size() + 1};

  XLOG(DBG7) << "FUSE_RENAME " << oldName << " -> " << newName;
  return dispatcher_->rename(header->nodeid, oldName, rename->newdir, newName)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseLink(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto link = reinterpret_cast<const fuse_link_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(link + 1);
  PathComponentPiece newName{nameStr};

  XLOG(DBG7) << "FUSE_LINK " << newName;

  return dispatcher_->link(link->oldnodeid, header->nodeid, newName)
      .then([](fuse_entry_out param) { RequestData::get().sendReply(param); });
}

folly::Future<folly::Unit> FuseChannel::fuseOpen(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPEN";
  return dispatcher_->open(header->nodeid, open->flags)
      .then([this](std::shared_ptr<FileHandle> fh) {
        if (!fh) {
          throw std::runtime_error("Dispatcher::open failed to set fh");
        }
        fuse_open_out out;
        memset(&out, 0, sizeof(out));
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
  return dispatcher_->statfs(header->nodeid).then([](struct statvfs&& info) {
    fuse_statfs_out out;
    memset(&out, 0, sizeof(out));
    out.st.blocks = info.f_blocks;
    out.st.bfree = info.f_bfree;
    out.st.bavail = info.f_bavail;
    out.st.files = info.f_files;
    out.st.ffree = info.f_ffree;
    out.st.bsize = info.f_bsize;
    out.st.namelen = info.f_namemax;
    out.st.frsize = info.f_frsize;
    RequestData::get().sendReply(out);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseRelease(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto release = reinterpret_cast<const fuse_release_in*>(arg);
  XLOG(DBG7) << "FUSE_RELEASE";
  dispatcher_->getFileHandles().forgetGenericHandle(release->fh);
  RequestData::get().replyError(0);
  return Unit{};
}

folly::Future<folly::Unit> FuseChannel::fuseFsync(

    const fuse_in_header* header,
    const uint8_t* arg) {
  auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNC";

  auto fh = dispatcher_->getFileHandle(fsync->fh);
  return fh->fsync(datasync).then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseSetXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto setxattr = reinterpret_cast<const fuse_setxattr_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(setxattr + 1);
  StringPiece attrName{nameStr};
  auto bufPtr = nameStr + attrName.size() + 1;
  StringPiece value(bufPtr, setxattr->size);

  XLOG(DBG7) << "FUSE_SETXATTR";

  return dispatcher_->setxattr(header->nodeid, attrName, value, setxattr->flags)
      .then([]() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseGetXAttr(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto getxattr = reinterpret_cast<const fuse_getxattr_in*>(arg);
  auto nameStr = reinterpret_cast<const char*>(getxattr + 1);
  StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_GETXATTR";
  return dispatcher_->getxattr(header->nodeid, attrName)
      .then([size = getxattr->size](std::string attr) {
        auto& request = RequestData::get();
        if (size == 0) {
          fuse_getxattr_out out;
          memset(&out, 0, sizeof(out));
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
  auto listattr = reinterpret_cast<const fuse_getxattr_in*>(arg);
  XLOG(DBG7) << "FUSE_LISTXATTR";
  return dispatcher_->listxattr(header->nodeid)
      .then([size = listattr->size](std::vector<std::string> attrs) {
        auto& request = RequestData::get();

        // Initialize count to include the \0 for each
        // entry.
        size_t count = attrs.size();
        for (auto& attr : attrs) {
          count += attr.size();
        }

        if (size == 0) {
          // caller is asking for the overall size
          fuse_getxattr_out out;
          memset(&out, 0, sizeof(out));
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
  auto nameStr = reinterpret_cast<const char*>(arg);
  StringPiece attrName{nameStr};
  XLOG(DBG7) << "FUSE_REMOVEXATTR";
  return dispatcher_->removexattr(header->nodeid, attrName).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseFlush(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto flush = reinterpret_cast<const fuse_flush_in*>(arg);
  XLOG(DBG7) << "FUSE_FLUSH";
  auto fh = dispatcher_->getFileHandle(flush->fh);

  return fh->flush(flush->lock_owner).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseOpenDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto open = reinterpret_cast<const fuse_open_in*>(arg);
  XLOG(DBG7) << "FUSE_OPENDIR";
  return dispatcher_->opendir(header->nodeid, open->flags)
      .then([this](std::shared_ptr<DirHandle> dh) {
        if (!dh) {
          throw std::runtime_error("Dispatcher::opendir failed to set dh");
        }
        fuse_open_out out;
        memset(&out, 0, sizeof(out));
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
  auto dh = dispatcher_->getDirHandle(read->fh);
  return dh->readdir(DirList(read->size), read->offset)
      .then([](DirList&& list) {
        auto buf = list.getBuf();
        RequestData::get().sendReply(StringPiece(buf));
      });
}

folly::Future<folly::Unit> FuseChannel::fuseReleaseDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto release = reinterpret_cast<const fuse_release_in*>(arg);
  XLOG(DBG7) << "FUSE_RELEASEDIR";
  dispatcher_->getFileHandles().forgetGenericHandle(release->fh);
  RequestData::get().replyError(0);
  return Unit{};
}

folly::Future<folly::Unit> FuseChannel::fuseFsyncDir(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto fsync = reinterpret_cast<const fuse_fsync_in*>(arg);
  // There's no symbolic constant for this :-/
  bool datasync = fsync->fsync_flags & 1;

  XLOG(DBG7) << "FUSE_FSYNCDIR";

  auto dh = dispatcher_->getDirHandle(fsync->fh);
  return dh->fsyncdir(datasync).then(
      []() { RequestData::get().replyError(0); });
}

folly::Future<folly::Unit> FuseChannel::fuseAccess(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto access = reinterpret_cast<const fuse_access_in*>(arg);
  XLOG(DBG7) << "FUSE_ACCESS";
  return dispatcher_->access(header->nodeid, access->mask).then([]() {
    RequestData::get().replyError(0);
  });
}

folly::Future<folly::Unit> FuseChannel::fuseCreate(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto create = reinterpret_cast<const fuse_create_in*>(arg);
  PathComponentPiece name{reinterpret_cast<const char*>(create + 1)};
  XLOG(DBG7) << "FUSE_CREATE " << name;
  return dispatcher_->create(header->nodeid, name, create->mode, create->flags)
      .then([this](Dispatcher::Create info) {

        fuse_open_out out;
        memset(&out, 0, sizeof(out));
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
  auto bmap = reinterpret_cast<const fuse_bmap_in*>(arg);
  XLOG(DBG7) << "FUSE_BMAP";
  return dispatcher_->bmap(header->nodeid, bmap->blocksize, bmap->block)
      .then([](uint64_t resultIdx) {
        fuse_bmap_out out;
        out.block = resultIdx;
        RequestData::get().sendReply(out);
      });
}

folly::Future<folly::Unit> FuseChannel::fuseBatchForget(
    const fuse_in_header* header,
    const uint8_t* arg) {
  auto forgets = reinterpret_cast<const fuse_batch_forget_in*>(arg);
  auto item = reinterpret_cast<const fuse_forget_one*>(forgets + 1);
  auto end = item + forgets->count;
  XLOG(DBG7) << "FUSE_BATCH_FORGET";

  while (item != end) {
    dispatcher_->forget(item->nodeid, item->nlookup);
    ++item;
  }
  return Unit{};
}

} // namespace fusell
} // namespace eden
} // namespace facebook
