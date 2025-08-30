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

#include "eden/common/utils/Throw.h"

namespace facebook::eden {
std::tuple<RootId, std::string> parseFilterIdFromRootId(const RootId& rootId) {
  if (rootId == RootId{}) {
    // Null root id. Just render the empty string(no filter)
    return {RootId{}, std::string()};
  }
  auto rootRange = folly::range(rootId.value());
  auto expectedLength = folly::tryDecodeVarint(rootRange);
  if (UNLIKELY(!expectedLength)) {
    throwf<std::invalid_argument>(
        "Could not decode varint; FilteredBackingStore expects a root ID in "
        "the form of <idLengthVarint><scmId><filterId>, got {}",
        rootId.value());
  }
  auto root = RootId{std::string{rootRange.begin(), expectedLength.value()}};
  auto filterId = std::string{rootRange.begin() + expectedLength.value()};
  XLOGF(
      DBG7,
      "Decoded Original RootId Length: {}, Original RootId: {}, FilterID: {}",
      expectedLength.value(),
      root.value(),
      filterId);
  return {std::move(root), std::move(filterId)};
}
} // namespace facebook::eden
