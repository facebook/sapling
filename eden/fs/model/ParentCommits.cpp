/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
