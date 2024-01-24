/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FilterUtils.h"

#include <folly/Range.h>
#include <folly/Varint.h>
#include <folly/logging/xlog.h>
#include <tuple>

#include "eden/fs/utils/Throw.h"

namespace facebook::eden {
std::tuple<RootId, std::string> parseFilterIdFromRootId(const RootId& rootId) {
  auto rootRange = folly::range(rootId.value());
  auto expectedLength = folly::tryDecodeVarint(rootRange);
  if (UNLIKELY(!expectedLength)) {
    throwf<std::invalid_argument>(
        "Could not decode varint; FilteredBackingStore expects a root ID in "
        "the form of <hashLengthVarint><scmHash><filterId>, got {}",
        rootId.value());
  }
  auto root = RootId{std::string{rootRange.begin(), expectedLength.value()}};
  auto filterId = std::string{rootRange.begin() + expectedLength.value()};
  XLOGF(
      DBG7,
      "Decoded Original RootId Length: {}, Original RootId: {}, FilterID: {}",
      expectedLength.value(),
      filterId,
      root.value());
  return {std::move(root), std::move(filterId)};
}
} // namespace facebook::eden
