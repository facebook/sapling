/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <iosfwd>
#include <optional>

#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

/**
 * Data about the parent commits for a working directory.
 *
 * In most circumstances there will only be a single parent, but there
 * will be two parents when in the middle of resolving a merge conflict.
 */
class ParentCommits {
 public:
  ParentCommits() = default;
  explicit ParentCommits(const Hash& p1) : parent1_{p1} {}
  explicit ParentCommits(const Hash& p1, const std::optional<Hash>& p2)
      : parent1_{p1}, parent2_{p2} {}

  Hash& parent1() {
    return parent1_;
  }
  const Hash& parent1() const {
    return parent1_;
  }

  std::optional<Hash>& parent2() {
    return parent2_;
  }
  const std::optional<Hash>& parent2() const {
    return parent2_;
  }

  void setParents(const Hash& p1) {
    parent1_ = p1;
    parent2_ = std::nullopt;
  }
  void setParents(const Hash& p1, const std::optional<Hash>& p2) {
    parent1_ = p1;
    parent2_ = p2;
  }
  void setParents(const ParentCommits& parents) {
    parent1_ = parents.parent1();
    parent2_ = parents.parent2();
  }

  // Copy constructor and copy assignment.
  // There isn't much point in having move-assignment or move construction,
  // since all of our data is stored inline, and can't really be moved.
  ParentCommits(const ParentCommits& other) = default;
  ParentCommits& operator=(const ParentCommits& other) = default;

  bool operator==(const ParentCommits& other) const;
  bool operator!=(const ParentCommits& other) const;

 private:
  Hash parent1_;
  std::optional<Hash> parent2_;
};

/**
 * Output stream operator for ParentCommits.
 *
 * This makes it possible to easily use ParentCommits in glog statements.
 */
std::ostream& operator<<(std::ostream& os, const ParentCommits& parents);
} // namespace eden
} // namespace facebook
