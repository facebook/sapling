/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/futures/Future.h>
#include "eden/fs/fuse/BufVec.h"
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/PollHandle.h"

namespace facebook {
namespace eden {

class Dispatcher;

class FileHandleBase {
 public:
  virtual ~FileHandleBase();

  /* The result of an ioctl operation */
  struct Ioctl {
    int result;
    BufVec buf;
  };

  /**
   * Ioctl
   *
   * Only well-formed (restricted) ioctls are supported.  These are ioctls
   * that have the argument size encoded using _IOR, _IOW, _IOWR macros.
   *
   * @param arg is the argument passed in from userspace
   * @param inputData is a copy of the arg data from userspace
   * @param outputSize is the maximum size of the output data
   */
  virtual folly::Future<Ioctl> ioctl(
      int cmd,
      const void* arg,
      folly::ByteRange inputData,
      size_t outputSize);

  /**
   * Poll for IO readiness
   *
   * Introduced in version 2.8
   *
   * Note: If ph is non-NULL, the client should notify
   * when IO readiness events occur by calling
   * ph->notify().
   *
   * Regardless of the number of times poll with a non-NULL ph
   * is received, single notification is enough to clear all.
   * Notifying more times incurs overhead but doesn't harm
   * correctness.
   *
   * Return the poll(2) revents mask.
   */
  virtual folly::Future<unsigned> poll(std::unique_ptr<PollHandle> ph);
};

} // namespace eden
} // namespace facebook
