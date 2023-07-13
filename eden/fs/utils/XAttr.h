/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <errno.h>
#include <string>
#include <string_view>
#include <vector>

#ifndef _WIN32
#include <sys/xattr.h>
#endif

namespace facebook::eden {

#ifndef _WIN32

constexpr int kENOATTR =
#ifndef ENOATTR
    ENODATA // Linux
#else
    ENOATTR
#endif
    ;

constexpr std::string_view kXattrSha1{"user.sha1"};
constexpr std::string_view kXattrBlake3{"user.blake3"};

std::string fgetxattr(int fd, std::string_view name);
void fsetxattr(int fd, std::string_view name, std::string_view value);

/// like getxattr(2), but portable. This is primarily to facilitate our
/// integration tests.
std::string getxattr(std::string_view path, std::string_view name);

/// like listxattr(2), but more easily consumable from C++.
// This is primarily to facilitate our integration tests.
std::vector<std::string> listxattr(std::string_view path);

#endif

} // namespace facebook::eden
