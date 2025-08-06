/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/RootId.h"
#include <folly/String.h>
#include <string>

namespace facebook::eden {

void toAppend(const RootId& rootId, std::string* result) {
  folly::cEscape(rootId.value(), *result);
}

} // namespace facebook::eden
