/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PollHandle.h"

namespace facebook {
namespace eden {
namespace fusell {

void PollHandle::Deleter::operator()(fuse_pollhandle* h) {
#if FUSE_MAJOR_VERSION >= 8
  fuse_pollhandle_destroy(h);
#endif
}

PollHandle::PollHandle(fuse_pollhandle* h) : h_(h, PollHandle::Deleter()) {}

void PollHandle::notify() {
#if FUSE_MAJOR_VERSION >= 8
  fuse_lowlevel_notify_poll(h_.get());
#endif
}
}
}
}
