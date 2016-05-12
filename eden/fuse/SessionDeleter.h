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
#include "Channel.h"
#include "eden/fuse/fuse_headers.h"

namespace facebook {
namespace eden {
namespace fusell {

class SessionDeleter {
  Channel* chan_;

 public:
  explicit SessionDeleter(Channel* chan);
  void operator()(fuse_session*);
};
}
}
}
