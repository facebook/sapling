/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/KeySpace.h"

namespace facebook::eden {

// Older versions of MSVC++ ICE on the following code.
#if !defined(_MSC_FULL_VER) || _MSC_FULL_VER >= 192027508

namespace {
constexpr bool assertKeySpaceInvariants() {
  size_t index = 0;
  for (auto& ks : KeySpace::kAll) {
    if (index != ks->index) {
      return false;
    }
    index += 1;
  }
  return index == KeySpace::kTotalCount;
}
} // namespace

static_assert(assertKeySpaceInvariants());

#endif

} // namespace facebook::eden
