/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/PathMap.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeAuxDataFwd.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/TreeFwd.h"

namespace facebook::eden {

class Tree;
using TreePtr = std::shared_ptr<const Tree>;

class Tree {
 public:
  using container = PathMap<TreeEntry>;
  using key_type = container::key_type;
  using mapped_type = container::mapped_type;
  using value_type = container::value_type;
  using const_iterator = container::const_iterator;

  /**
   * Construct a Tree.
   *
   * Temporarily takes a CaseSensitivity argument default initialized. In the
   * future, once all the callers are updated to pass the correct
   * CaseSensitivity, the default value will be removed.
   *
   * In the case where kPathMapDefaultCaseSensitive is not the same as the
   * mount case sensitivity, the caller is responsible for constructing a new
   * Tree with the case sensitivity flipped.
   */
  explicit Tree(
      container entries,
      ObjectId id,
      AclRootState state = AclRootState::Unknown)
      : id_{std::move(id)},
        entries_{std::move(entries)},
        aclRootState_{state} {}

  explicit Tree(
      ObjectId id,
      container entries,
      TreeAuxDataPtr auxData,
      AclRootState state = AclRootState::Unknown)
      : id_{std::move(id)},
        entries_{std::move(entries)},
        auxData_(std::move(auxData)),
        aclRootState_{state} {}

  /**
   * Construct a restricted tree. This is an empty tree that indicates the
   * server denied access to its contents via ACL.
   */
  struct Restricted {};
  explicit Tree(Restricted, container entries, ObjectId id)
      : id_{std::move(id)},
        entries_{std::move(entries)},
        aclRootState_{AclRootState::RestrictedAclRoot} {}

  TreePtr withNewId(container entries, ObjectId newId) const;

  TreePtr withNewId(ObjectId newId) const;

  const ObjectId& getObjectId() const {
    return id_;
  }

  const TreeAuxDataPtr getAuxData() const {
    return auxData_;
  }

  /**
   * An estimate of the memory footprint of this tree. Called by ObjectCache to
   * limit the number of cached trees in memory at a time.
   */
  size_t getSizeBytes() const;

  /**
   * Find an entry in this Tree whose name match the passed in path.
   */
  const_iterator find(PathComponentPiece name) const {
    return entries_.find(name);
  }

  const_iterator cbegin() const {
    return entries_.cbegin();
  }

  const_iterator begin() const {
    return cbegin();
  }

  const_iterator cend() const {
    return entries_.cend();
  }

  const_iterator end() const {
    return cend();
  }

  size_t size() const {
    return entries_.size();
  }

  const container& entries() const {
    return entries_;
  }

  /**
   * Returns the case sensitivity of this tree.
   */
  CaseSensitivity getCaseSensitivity() const {
    return entries_.getCaseSensitivity();
  }

  /**
   * Returns true if this tree represents a directory the server denied
   * access to via ACL restrictions.
   */
  bool isRestricted() const {
    return aclRootState_ == AclRootState::RestrictedAclRoot;
  }

  /**
   * Returns true if this tree is structurally covered by an ACL root.
   * This is independent from whether the caller currently has access.
   */
  std::optional<bool> hasACL() const {
    return hasACLFromAclRootState(aclRootState_);
  }

  AclRootState aclRootState() const {
    return aclRootState_;
  }

 private:
  friend bool operator==(const Tree& tree1, const Tree& tree2);

  ObjectId id_;
  container entries_;
  TreeAuxDataPtr auxData_;
  AclRootState aclRootState_{AclRootState::Unknown};
};

} // namespace facebook::eden
