/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/SocketAddress.h>
#include <optional>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

folly::SocketAddress makeNfsSocket(std::optional<AbsolutePath> unixSocketPath);

}
