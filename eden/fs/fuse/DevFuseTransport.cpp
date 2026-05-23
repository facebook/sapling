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

#include <folly/logging/xlog.h>

#include <unistd.h>
#include <limits>

namespace facebook::eden {

const char* DevFuseTransport::getName() const {
  return "devfuse";
}

size_t DevFuseTransport::getWorkerThreadCount(size_t defaultThreadCount) const {
  return defaultThreadCount;
}

ssize_t DevFuseTransport::readInitPacket(int fd, void* buf, size_t size) const {
  return read(fd, buf, size);
}

void DevFuseTransport::processSession(FuseChannel& channel) {
  std::vector<char> buf(channel.getTransportBufferSize());
  // Save this for the sanity check later in the loop to avoid
  // additional syscalls on each loop iteration.
  auto myPid = getpid();

  while (!channel.isStopRequested()) {
    // TODO: FUSE_SPLICE_READ allows using splice(2) here if we enable it.
    // We can look at turning this on once the main plumbing is complete.
    auto res = read(channel.getFuseDeviceFd(), buf.data(), buf.size());
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

    channel.dispatchRequestFromTransport(*header, arg, myPid);
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
