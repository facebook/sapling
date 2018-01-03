/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#ifdef __linux__
#include "eden/third-party/fuse_kernel_linux.h"
#else
#error need a fuse kernel header to be included for your OS!
#endif
namespace facebook {
namespace eden {
namespace fusell {

/** Represents ino_t in a differently portable way */
using InodeNumber = uint64_t;

} // namespace fusell
} // namespace eden
} // namespace facebook
