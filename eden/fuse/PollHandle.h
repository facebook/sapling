/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "fuse_headers.h"
#include <memory>

namespace facebook {
namespace eden {
namespace fusell {

// Some compatibility cruft for working with OSX Fuse
#if FUSE_MINOR_VERSION < 8
typedef void* fuse_pollhandle;
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
}
}
}
