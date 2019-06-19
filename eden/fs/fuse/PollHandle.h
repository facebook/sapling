/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <memory>
#include "eden/fs/fuse/FuseTypes.h"

namespace facebook {
namespace eden {

// Some compatibility cruft for working with OSX Fuse
#if FUSE_MINOR_VERSION < 8
using fuse_pollhandle = void*;
#endif

class PollHandle {
  struct Deleter {
    void operator()(fuse_pollhandle*);
  };
  std::unique_ptr<fuse_pollhandle, Deleter> h_;

 public:
  PollHandle(const PollHandle&) = delete;
  PollHandle& operator=(const PollHandle&) = delete;
  PollHandle(PollHandle&&) = default;
  PollHandle& operator=(PollHandle&&) = default;

  explicit PollHandle(fuse_pollhandle* h);

  // Requests that the kernel poll the associated file
  void notify();
};

} // namespace eden
} // namespace facebook
