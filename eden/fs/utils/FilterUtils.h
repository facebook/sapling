/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/fs/model/RootId.h"

namespace facebook::eden {
/*
 * Splits FilteredRootIds into two parts: a FilterID and the original underlying
 * RootId. This util function is mainly for use in the FilteredBackingStore.
 * Some other parts of the codebase need this logic (and don't have access to a
 * FilteredBackingStore), so we put it in a util funciton for wider use.
 */
std::tuple<RootId, std::string> parseFilterIdFromRootId(const RootId& rootId);
} // namespace facebook::eden
