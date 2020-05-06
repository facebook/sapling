/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef _WIN32
#include "eden/fs/win/utils/FileUtils.h" // @manual
#else
#include <folly/FileUtil.h>

namespace facebook {
namespace eden {
using folly::writeFile;
using folly::writeFileAtomic;
} // namespace eden
} // namespace facebook
#endif
