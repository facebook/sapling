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
void fsetxattr(int fd, folly::StringPiece name, folly::StringPiece value);

/// like getxattr(2), but portable. This is primarily to facilitate our
/// integration tests.
std::string getxattr(folly::StringPiece path, folly::StringPiece name);

/// like listxattr(2), but more easily consumable from C++.
// This is primarily to facilitate our integration tests.
std::vector<std::string> listxattr(folly::StringPiece path);

} // namespace eden
} // namespace facebook
