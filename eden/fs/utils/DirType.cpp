/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/DirType.h"
#include <folly/Utility.h>
#include "eden/fs/service/gen-cpp2/eden_types.h"

using folly::to_underlying;

namespace facebook::eden {

/**
 * In theory, each platform could define its own values. In reality, Darwin,
 * FreeBSD, Linux, and the Windows CRT POSIX emulation layer use the same
 * values, so assert that they line up with our Thrift enumeration.
 */
static_assert(to_underlying(Dtype::UNKNOWN) == DT_UNKNOWN);
static_assert(to_underlying(Dtype::FIFO) == DT_FIFO);
static_assert(to_underlying(Dtype::CHAR) == DT_CHR);
static_assert(to_underlying(Dtype::DIR) == DT_DIR);
static_assert(to_underlying(Dtype::REGULAR) == DT_REG);
#ifndef _WIN32
static_assert(to_underlying(Dtype::BLOCK) == DT_BLK);
static_assert(to_underlying(Dtype::LINK) == DT_LNK);
static_assert(to_underlying(Dtype::SOCKET) == DT_SOCK);
static_assert(to_underlying(Dtype::WHITEOUT) == DT_WHT);
#endif

} // namespace facebook::eden
