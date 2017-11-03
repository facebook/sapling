/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/File.h>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>
#include "eden/fs/fuse/fuse_headers.h"
#include "eden/fs/utils/PathFuncs.h"
namespace facebook {
namespace eden {
namespace fusell {

class Dispatcher;

class FuseChannel {
 public:
  ~FuseChannel();

  /**
   * Construct the fuse channel and session structures that are
   * required by libfuse to communicate with the kernel using
   * a pre-existing fuseDevice descriptor.  The descriptor may
   * have been obtained via privilegedFuseMount() or may have
   * been passed to us as part of a graceful restart procedure.
   */
  FuseChannel(
      folly::File&& fuseDevice,
      bool debug,
      Dispatcher* const dispatcher);

  // Forbidden copy constructor and assignment operator
  FuseChannel(FuseChannel const&) = delete;
  FuseChannel& operator=(FuseChannel const&) = delete;

  /**
   * Dispatches fuse requests until the session is torn down.
   * This function blocks until the fuse session is stopped.
   * The intent is that this is called from each of the
   * fuse worker threads provided by the MountPoint. */
  void processSession();

  /**
   * Requests that the worker threads terminate their processing loop.
   */
  void requestSessionExit();

  /**
   * When performing a graceful restart, extract the fuse device
   * descriptor from the channel, preventing it from being closed
   * when we destroy this channel instance.
   * Note that this method does not prevent the worker threads
   * from continuing to use the fuse session.
   */
  folly::File stealFuseDevice();

  /**
   * Notify to invalidate cache for an inode
   *
   * @param ino the inode number
   * @param off the offset in the inode where to start invalidating
   *            or negative to invalidate attributes only
   * @param len the amount of cache to invalidate or 0 for all
   */
  void invalidateInode(fuse_ino_t ino, off_t off, off_t len);

  /**
   * Notify to invalidate parent attributes and the dentry matching
   * parent/name
   *
   * @param parent inode number
   * @param name file name
   */
  void invalidateEntry(fuse_ino_t parent, PathComponentPiece name);

 private:
  /*
   * fuse_chan_ops functions.
   *
   * These are very similar to the ones defined in libfuse.
   * Unfortunately libfuse does not provide a public API for creating a channel
   * from a mounted /dev/fuse file descriptor, so we have to provide our own
   * implementations.
   */
  static int recv(struct fuse_chan** chp, char* buf, size_t size);
  static int send(struct fuse_chan* ch, const struct iovec iov[], size_t count);
  static void destroy(struct fuse_chan* ch);

  fuse_chan* ch_{nullptr};
  fuse_args args_{0, nullptr, 0};
  struct fuse_session* session_{nullptr};
  Dispatcher* const dispatcher_{nullptr};
  folly::File fuseDevice_;
};
} // namespace fusell
} // namespace eden
} // namespace facebook
