/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/FBString.h>
#include <string>

namespace facebook {
namespace eden {
size_t estimateIndirectMemoryUsage(const std::string& path);
size_t estimateIndirectMemoryUsage(const folly::fbstring& path);
} // namespace eden
} // namespace facebook
