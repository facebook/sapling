/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Channel.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <linux/fuse.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <unistd.h>
#include <algorithm>

#include "Dispatcher.h"
#include "MountPoint.h"
#include "SessionDeleter.h"
#include "eden/fuse/privhelper/PrivHelper.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

namespace {

/*
 * fuse_chan_ops functions.
 *
 * These are very similar to the ones defined in libfuse.
 * Unfortunately libfuse does not provide a public API for creating a channel
 * from a mounted /dev/fuse file descriptor, so we have to provide our own
 * implementations.
 */

int fuseChanReceive(struct fuse_chan** chp, char* buf, size_t size) {
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
        LOG(WARNING) << "error reading from fuse channel: "
                     << folly::errnoStr(err);
      }
      return -err;
    }

    // It really seems like our caller should be responsible for
    // checking that a short read wasn't performed before using the buffer,
    // rather than just assuming that the receive operator will always do this.
    //
    // Unfortunately it doesn't look like fuse_do_work() checks the buffer
    // length before using header fields though, so we have to make sure to
    // check for this ourselves.
    if (static_cast<size_t>(res) < sizeof(struct fuse_in_header)) {
      LOG(ERROR) << "read truncated message from kernel fuse device: len="
                 << res;
      return -EIO;
    }
    return res;
  }
}

int fuseChanSend(struct fuse_chan* ch, const struct iovec iov[], size_t count) {
  if (!iov) {
    return 0;
  }

  int fd = fuse_chan_fd(ch);
  auto res = writev(fd, iov, count);
  int err = errno;
  if (res < 0) {
    if (err == ENOENT) {
      // Interrupted by a signal.  This is not an issue
    } else if (fuse_session_exited(fuse_chan_session(ch))) {
      LOG(INFO) << "error writing to fuse device: session closed";
    } else {
      LOG(WARNING) << "error writing to fuse device: " << folly::errnoStr(err);
    }
    return -err;
  }
  return 0;
}

void fuseChanDestroy(struct fuse_chan* ch) {
  close(fuse_chan_fd(ch));
}

fuse_chan* fuseChanNew(folly::File&& fuseDevice) {
  struct fuse_chan_ops op;
  op.receive = fuseChanReceive;
  op.send = fuseChanSend;
  op.destroy = fuseChanDestroy;

  constexpr size_t MIN_BUFSIZE = 0x21000;
  size_t bufsize =
      std::min(static_cast<size_t>(getpagesize()) + 0x1000, MIN_BUFSIZE);
  auto* ch = fuse_chan_new(&op, fuseDevice.fd(), bufsize, nullptr);
  if (!ch) {
    throw std::runtime_error("failed to mount");
  }
  // fuse_chan_new() takes ownership of the file descriptor, so call
  // fuseDevice.release() now.  The fd will be closed in fuseChanDestroy()
  // when the channel is destroyed.
  fuseDevice.release();

  return ch;
}

} // unnamed namespace

Channel::Channel(const MountPoint* mount) : mountPoint_(mount) {
  auto fuseDevice = privilegedFuseMount(mountPoint_->getPath().stringPiece());
  ch_ = fuseChanNew(std::move(fuseDevice));
}

const MountPoint* Channel::getMountPoint() const {
  return mountPoint_;
}

Channel::~Channel() {
  if (ch_) {
    fuse_unmount(mountPoint_->getPath().c_str(), ch_);
  }
}

void Channel::invalidateInode(fuse_ino_t ino, off_t off, off_t len) {
#if FUSE_MINOR_VERSION >= 8
  checkKernelError(fuse_lowlevel_notify_inval_inode(ch_, ino, off, len));
#endif
}

void Channel::invalidateEntry(fuse_ino_t parent, PathComponentPiece name) {
#if FUSE_MINOR_VERSION >= 8
  auto namePiece = name.stringPiece();
  checkKernelError(fuse_lowlevel_notify_inval_entry(
      ch_, parent, namePiece.data(), namePiece.size()));
#endif
}

void Channel::runSession(Dispatcher* disp, bool debug) {
  auto sess = disp->makeSession(*this, debug);
  fuse_session_add_chan(sess.get(), ch_);

  auto err = fuse_session_loop_mt(sess.get());
  if (err) {
    throw std::runtime_error("session failed");
  }
  LOG(INFO) << "session completed";
}
}
}
}
