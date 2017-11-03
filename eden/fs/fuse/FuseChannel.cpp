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
#include "eden/fs/fuse/Dispatcher.h"

#ifdef __linux__
#include <linux/fuse.h>
#endif

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

/*
 * fuse_chan_ops functions.
 *
 * These are very similar to the ones defined in libfuse.
 * Unfortunately libfuse does not provide a public API for creating a channel
 * from a mounted /dev/fuse file descriptor, so we have to provide our own
 * implementations.
 */

int FuseChannel::recv(struct fuse_chan** chp, char* buf, size_t size) {
  struct fuse_chan* ch = *chp;
  auto session = fuse_chan_session(ch);

  int fd = fuse_chan_fd(ch);
  while (true) {
    auto res = read(fd, buf, size);
    int err = errno;

    if (fuse_session_exited(session)) {
      return 0;
    }
    if (res < 0) {
      if (err == ENOENT) {
        // According to comments in the libfuse code:
        // ENOENT means the operation was interrupted; it's safe to restart
        continue;
      }
      if (err == ENODEV) {
        // ENODEV means the filesystem was unmounted
        fuse_session_exit(session);
        return 0;
      }
      if (err != EINTR && err != EAGAIN) {
        XLOG(WARNING) << "error reading from fuse channel: "
                      << folly::errnoStr(err);
      }
      return -err;
    }

#ifdef __linux__
    // It really seems like our caller should be responsible for
    // checking that a short read wasn't performed before using the buffer,
    // rather than just assuming that the receive operator will always do this.
    //
    // Unfortunately it doesn't look like fuse_do_work() checks the buffer
    // length before using header fields though, so we have to make sure to
    // check for this ourselves.
    //
    // This check is linux only because fuse_in_header is not exposed
    // to userspace on macOS.
    if (static_cast<size_t>(res) < sizeof(struct fuse_in_header)) {
      XLOG(ERR) << "read truncated message from kernel fuse device: len="
                << res;
      return -EIO;
    }
#endif

    return res;
  }
}

int FuseChannel::send(
    struct fuse_chan* ch,
    const struct iovec iov[],
    size_t count) {
  if (!iov) {
    return 0;
  }

  int fd = fuse_chan_fd(ch);
  auto res = writev(fd, iov, count);
  int err = errno;
  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal.  We don't need to log this,
      // but will propagate it back to our caller.
    } else if (fuse_session_exited(fuse_chan_session(ch))) {
      XLOG(INFO) << "error writing to fuse device: session closed";
    } else {
      XLOG(WARNING) << "error writing to fuse device: " << folly::errnoStr(err);
    }
    return -err;
  }
  return 0;
}

void FuseChannel::destroy(struct fuse_chan*) {
  // Closing the descriptor is managed entirely by the FuseChannel,
  // so we have nothing to do here.
}

FuseChannel::FuseChannel(
    folly::File&& fuseDevice,
    bool debug,
    Dispatcher* const dispatcher)
    : dispatcher_(dispatcher), fuseDevice_(std::move(fuseDevice)) {
  struct fuse_chan_ops op;
  op.receive = recv;
  op.send = send;
  op.destroy = destroy;

  // This is the minimum size used by libfuse for unspecified reasons,
  // so we use it too!
  constexpr size_t MIN_BUFSIZE = 0x21000;
  size_t bufsize =
      std::min(static_cast<size_t>(getpagesize()) + 0x1000, MIN_BUFSIZE);
  ch_ = fuse_chan_new(&op, fuseDevice_.fd(), bufsize, nullptr);
  if (!ch_) {
    throw std::runtime_error("failed to mount");
  }

  fuse_opt_add_arg(&args_, "fuse");
  fuse_opt_add_arg(&args_, "-o");
  fuse_opt_add_arg(&args_, "allow_root");
  if (debug) {
    fuse_opt_add_arg(&args_, "-d");
  }

  session_ = fuse_lowlevel_new(
      &args_, &dispatcher_ops, sizeof(dispatcher_ops), dispatcher_);
  if (!session_) {
    throw std::runtime_error("failed to create session");
  }

  fuse_session_add_chan(session_, ch_);
}

FuseChannel::~FuseChannel() {
  if (ch_) {
    // Prevents fuse_session_destroy() from destroying channel;
    // we want to do that explicitly.
    fuse_session_remove_chan(ch_);
  }
  if (session_) {
    fuse_session_destroy(session_);
  }
  if (ch_) {
    fuse_chan_destroy(ch_);
  }
}

folly::File FuseChannel::stealFuseDevice() {
  // Claim the fd
  folly::File fd;
  std::swap(fd, fuseDevice_);

  return fd;
}

void FuseChannel::invalidateInode(fuse_ino_t ino, off_t off, off_t len) {
#if FUSE_MINOR_VERSION >= 8
  int err = fuse_lowlevel_notify_inval_inode(ch_, ino, off, len);
  // Ignore ENOENT.  This can happen for inode numbers that we allocated on our
  // own and haven't actually told the kernel about yet.
  if (err != 0 && err != -ENOENT) {
    throwSystemErrorExplicit(-err, "error invalidating FUSE inode ", ino);
  }
#endif
}

void FuseChannel::invalidateEntry(fuse_ino_t parent, PathComponentPiece name) {
#if FUSE_MINOR_VERSION >= 8
  auto namePiece = name.stringPiece();
  int err = fuse_lowlevel_notify_inval_entry(
      ch_, parent, namePiece.data(), namePiece.size());
  // Ignore ENOENT.  This can happen for inode numbers that we allocated on our
  // own and haven't actually told the kernel about yet.
  if (err != 0 && err != -ENOENT) {
    throwSystemErrorExplicit(
        -err,
        "error invalidating FUSE entry ",
        name,
        " in directory inode ",
        parent);
  }
#endif
}

void FuseChannel::requestSessionExit() {
  fuse_session_exit(session_);
}

void FuseChannel::processSession() {
  std::vector<char> buf;
  buf.resize(fuse_chan_bufsize(ch_));

  while (!fuse_session_exited(session_)) {
    struct fuse_chan* ch = ch_;

    auto res = fuse_chan_recv(&ch, buf.data(), buf.size());
    if (res == -EINTR) {
      // If we got interrupted by a signal while reading the next
      // fuse command, we will simply retry and read the next thing.
      continue;
    }

    if (res <= 0) {
      if (res < 0) {
        fuse_session_exit(session_);
      }
      continue;
    }

    fuse_session_process(session_, buf.data(), res, ch);
  }
}
} // namespace fusell
} // namespace eden
} // namespace facebook
