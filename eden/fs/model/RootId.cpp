/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/RootId.h"
#include <folly/String.h>

namespace facebook::eden {

std::ostream& operator<<(std::ostream& os, const RootId& rootId) {
  return os << rootId.value();
}

void toAppend(const RootId& rootId, std::string* result) {
  folly::cEscape(rootId.value(), *result);
}

} // namespace facebook::eden
