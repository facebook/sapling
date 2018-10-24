/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/io/async/SSLContext.h>
#include <optional>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {
/**
 * Create a folly::SSLcontext with client certificate
 */
std::shared_ptr<folly::SSLContext> buildSSLContext(
    std::optional<AbsolutePath> clientCertificate);
} // namespace eden
} // namespace facebook
