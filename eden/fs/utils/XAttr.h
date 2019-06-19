/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
