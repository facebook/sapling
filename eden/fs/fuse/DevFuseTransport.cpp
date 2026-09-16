/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/DevFuseTransport.h"

#include "eden/common/utils/SystemError.h"
#include "eden/fs/fuse/FuseChannel.h"

#include <folly/ScopeGuard.h>
#include <folly/logging/xlog.h>

#include <fcntl.h>
#include <poll.h>
#include <unistd.h>
#include <cerrno>
#include <limits>

namespace facebook::eden {

const char* DevFuseTransport::getName() const {
  return kDevFuseTransportName;
}

size_t DevFuseTransport::getWorkerThreadCount(size_t defaultThreadCount) const {
  return defaultThreadCount;
}

ssize_t DevFuseTransport::readInitPacket(int fd, void* buf, size_t size) const {
  return read(fd, buf, size);
}

void DevFuseTransport::processSession(FuseChannel& channel) {
  processSession(channel, -1, [] {});
}

void DevFuseTransport::processSession(
    FuseChannel& channel,
    int stopFd,
    folly::FunctionRef<void()> onReady) {
  std::vector<char> buf(channel.getTransportBufferSize());
  const auto fuseFd = channel.getFuseDeviceFd();
  auto fcntlRetry = [fuseFd](int command, int flags = 0) {
    int result;
    do {
      result = fcntl(fuseFd, command, flags);
    } while (result < 0 && errno == EINTR);
    return result;
  };
  int originalFlags = -1;
  SCOPE_EXIT {
    if (originalFlags >= 0 && fcntlRetry(F_SETFL, originalFlags) < 0) {
      XLOGF(
          ERR,
          "failed to restore FUSE device flags after companion reader: {}",
          folly::errnoStr(errno));
    }
  };
  if (stopFd >= 0) {
    const auto flags = fcntlRetry(F_GETFL);
    if (flags < 0) {
      folly::throwSystemError("failed to read FUSE device flags");
    }
    // poll readiness can disappear before read(), including when the kernel
    // removes an interrupted request. The read must not block after a stop.
    if (fcntlRetry(F_SETFL, flags | O_NONBLOCK) < 0) {
      folly::throwSystemError(
          "failed to make companion FUSE reader nonblocking");
    }
    originalFlags = flags;
  }
  // Save this for the sanity check later in the loop to avoid
  // additional syscalls on each loop iteration.
  auto myPid = getpid();
  onReady();

  while (!channel.isStopRequested()) {
    if (stopFd >= 0) {
      pollfd fds[] = {{fuseFd, POLLIN, 0}, {stopFd, POLLIN, 0}};
      if (poll(fds, 2, -1) < 0) {
        if (errno == EINTR) {
          continue;
        }
        folly::throwSystemError("failed to poll companion FUSE reader");
      }
      if (channel.isStopRequested()) {
        break;
      }
      if (fds[1].revents & (POLLERR | POLLHUP | POLLNVAL)) {
        throw std::runtime_error("companion FUSE reader wakeup fd failed");
      }
      if (fds[1].revents & POLLIN) {
        uint64_t value;
        ssize_t result;
        do {
          result = read(stopFd, &value, sizeof(value));
        } while (result < 0 && errno == EINTR);
        if (result < 0 && errno != EAGAIN) {
          folly::throwSystemError("failed to drain companion FUSE wakeup fd");
        }
        if (result >= 0 && static_cast<size_t>(result) != sizeof(value)) {
          throw std::runtime_error("short read from companion FUSE wakeup fd");
        }
      }
      if (fds[0].revents & POLLNVAL) {
        folly::throwSystemErrorExplicit(
            EBADF, "invalid device fd for companion FUSE reader");
      }
      if (!(fds[0].revents & (POLLIN | POLLHUP | POLLERR))) {
        continue;
      }
    }
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(fuseFd, buf.data(), buf.size());
    if (res < 0) {
      int error = errno;
      if (channel.isStopRequested()) {
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
        channel.logUnmountEventAndExit();
        break;
      } else {
        XLOGF(
            WARNING,
            "error reading from fuse channel: {}",
            folly::errnoStr(error));
        channel.requestSessionExitFromTransport(
            FuseChannel::StopReason::FUSE_READ_ERROR);
        break;
      }
    }

    const auto argSize = static_cast<size_t>(res);
    if (argSize < sizeof(fuse_in_header)) {
      if (argSize == 0) {
        // This code path is hit when a fake FUSE channel is closed in our unit
        // tests. On real FUSE channels we should get ENODEV to indicate that
        // the FUSE channel was shut down. However, in our unit tests that use
        // fake FUSE connections we cannot send an ENODEV error, and so we just
        // close the channel instead.
        channel.requestSessionExitFromTransport(
            FuseChannel::StopReason::UNMOUNTED);
      } else {
        // We got a partial FUSE header. This should not happen unless there is
        // a bug in the FUSE kernel code.
        XLOGF(
            ERR,
            "read truncated message from kernel fuse device: len={}",
            argSize);
        channel.requestSessionExitFromTransport(
            FuseChannel::StopReason::FUSE_TRUNCATED_REQUEST);
      }
      return;
    }

    const auto* header = reinterpret_cast<fuse_in_header*>(buf.data());
    const folly::ByteRange arg{
        reinterpret_cast<const uint8_t*>(header + 1),
        argSize - sizeof(fuse_in_header)};

    // A successfully dequeued FORGET cannot be retried, even if stop raced
    // with read(). Dispatch it before leaving the reader.
    channel.dispatchRequestFromTransport(*this, *header, arg, myPid);
  }
}

void DevFuseTransport::replyError(
    FuseChannel& channel,
    const fuse_in_header& request,
    int errorCode) const {
  fuse_out_header err{};
  err.len = sizeof(err);
  err.error = -errorCode;
  err.unique = request.unique;
  XLOGF(
      DBG7,
      "replyError unique={} error={} {}",
      err.unique,
      errorCode,
      folly::errnoStr(errorCode));
  auto res = write(channel.getFuseDeviceFd(), &err, sizeof(err));
  if (res != sizeof(err)) {
    if (res < 0) {
      folly::throwSystemError("replyError: error writing to fuse device");
    } else {
      throw std::runtime_error("unexpected short write to FUSE device");
    }
  }
}

void DevFuseTransport::sendRawReply(
    FuseChannel& channel,
    const iovec iov[],
    size_t count) const {
  // Ensure that the length is set correctly
  XDCHECK_EQ(iov[0].iov_len, sizeof(fuse_out_header));
  const auto header = reinterpret_cast<fuse_out_header*>(iov[0].iov_base);
  header->len = 0;
  for (size_t i = 0; i < count; ++i) {
    header->len += iov[i].iov_len;
  }

  XDCHECK_LE(count, static_cast<size_t>(std::numeric_limits<int>::max()));
  const auto res =
      writev(channel.getFuseDeviceFd(), iov, static_cast<int>(count));
  const int err = errno;
  XLOGF(
      DBG7,
      "sendRawReply: unique={} header->len={} wrote={}",
      header->unique,
      header->len,
      res);

  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal. We don't need to log this, but will
      // propagate it back to our caller.
    } else if (!channel.isFuseDeviceValidForWrites()) {
      XLOG(INFO, "error writing to fuse device: session closed");
    } else {
      XLOGF(WARNING, "error writing to fuse device: {}", folly::errnoStr(err));
    }
    folly::throwSystemErrorExplicit(err, "error writing to fuse device");
  }
}

} // namespace facebook::eden

#endif
