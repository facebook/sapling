/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/portability/GFlags.h>
#include "eden/fs/utils/PathFuncs.h"

DECLARE_string(hgPath);

namespace facebook::eden {
AbsolutePath findAndConfigureHgBinary();
AbsolutePath findHgBinary();
} // namespace facebook::eden
