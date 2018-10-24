/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "ParentCommits.h"

#include <ostream>

namespace facebook {
namespace eden {

bool ParentCommits::operator==(const ParentCommits& other) const {
  return parent1_ == other.parent1_ && parent2_ == other.parent2_;
}

bool ParentCommits::operator!=(const ParentCommits& other) const {
  return !(*this == other);
}

std::ostream& operator<<(std::ostream& os, const ParentCommits& parents) {
  os << "[" << parents.parent1();
  if (parents.parent2().has_value()) {
    os << ", " << parents.parent2().value();
  }
  os << "]";
  return os;
}
} // namespace eden
} // namespace facebook
