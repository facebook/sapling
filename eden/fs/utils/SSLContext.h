/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
