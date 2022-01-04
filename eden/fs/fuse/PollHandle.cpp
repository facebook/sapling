/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/PollHandle.h"

namespace facebook::eden {

void PollHandle::Deleter::operator()(fuse_pollhandle* /*h*/) {
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

} // namespace facebook::eden

#endif
