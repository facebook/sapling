/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "SessionDeleter.h"

namespace facebook {
namespace eden {
namespace fusell {

SessionDeleter::SessionDeleter(Channel* chan) : chan_(chan) {}

void SessionDeleter::operator()(fuse_session* sess) {
  fuse_session_remove_chan(chan_->ch_);
  fuse_session_destroy(sess);
}
}
}
}
