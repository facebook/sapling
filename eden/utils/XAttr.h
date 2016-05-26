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
#include <errno.h>
#include <folly/String.h>
#include <sys/xattr.h>

namespace facebook {
namespace eden {

constexpr int kENOATTR =
#ifndef ENOATTR
    ENODATA // Linux
#else
    ENOATTR
#endif
    ;

constexpr folly::StringPiece kXattrSha1{"user.sha1"};

std::string fgetxattr(int fd, folly::StringPiece name);
}
}
