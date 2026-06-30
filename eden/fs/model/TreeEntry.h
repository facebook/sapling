/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <iosfwd>
#include <optional>
#include <string>
#include <vector>

#include <folly/Try.h>

#include "eden/common/utils/DirType.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {
class BlobAuxData;

/**
 * Represents the allowed types of entries in version control trees.
 *
 * Currently missing from this list: git submodules.
 */
enum class TreeEntryType : uint8_t {
  TREE,
  REGULAR_FILE,
  EXECUTABLE_FILE,
  SYMLINK,
};

enum class AclRootState : uint8_t {
  Unknown = 0,
  NoAcl = 1,
  AclRoot = 2,
  RestrictedAclRoot = 3,
};

inline std::optional<AclRootState> aclRootStateFromInt(int64_t value) {
  switch (value) {
    case 0:
      return AclRootState::Unknown;
    case 1:
      return AclRootState::NoAcl;
    case 2:
      return AclRootState::AclRoot;
    case 3:
      return AclRootState::RestrictedAclRoot;
    default:
      return std::nullopt;
  }
}

inline std::optional<bool> hasACLFromAclRootState(AclRootState state) {
  switch (state) {
    case AclRootState::Unknown:
      return std::nullopt;
    case AclRootState::NoAcl:
      return false;
    case AclRootState::AclRoot:
    case AclRootState::RestrictedAclRoot:
      return true;
  }
  return std::nullopt;
}

inline AclRootState makeAclRootState(
    bool isRestricted,
    std::optional<bool> hasACL) {
  if (isRestricted) {
    return AclRootState::RestrictedAclRoot;
  }
  if (!hasACL.has_value()) {
    return AclRootState::Unknown;
  }
  return *hasACL ? AclRootState::AclRoot : AclRootState::NoAcl;
}

/**
 * A single ACL entry for a path. Each entry represents one restriction root
 * with its associated repo-region ACL and optional request ACL.
 */
struct EntryAcl {
  std::string restrictionRoot;
  std::string repoRegionAcl;
  std::optional<std::string> requestAcl;

  bool operator==(const EntryAcl&) const = default;
};

/**
 * Access control metadata for a path. Mirrors the thrift AclInfo struct
 * but lives in the model layer without thrift dependencies.
 */
struct EntryAclInfo {
  bool underAcl{false};
  std::vector<EntryAcl> acls;

  bool operator==(const EntryAclInfo&) const = default;
};

struct EntryAttributes {
  // for each requested attribute the member here should be set. If the
  // attribute was not requested, then the member will be nullopt.
  // Any errors will be encapsulated in the try. For the Source Control type
  // member the inner optional may be nullopt, if the entry is not a source
  // control type. Currently, source control types only include directories,
  // regular files, executable files, and symlinks. FIFOs or sockets for
  // example would fall into the nullopt case.
  std::optional<folly::Try<Hash20>> sha1;
  std::optional<folly::Try<Hash32>> blake3;
  std::optional<folly::Try<uint64_t>> size;
  std::optional<folly::Try<std::optional<TreeEntryType>>> type;
  std::optional<folly::Try<std::optional<ObjectId>>> objectId;
  std::optional<folly::Try<uint64_t>> digestSize;
  std::optional<folly::Try<Hash32>> digestHash;
  std::optional<folly::Try<timespec>> mtime;
  std::optional<folly::Try<mode_t>> mode;
  std::optional<folly::Try<bool>> underAcl;
  std::optional<folly::Try<EntryAclInfo>> aclInfo;
};

/**
 * Comparing two EntryAttributes or Try of EntryAttributes, exceptions of any
 * kind are considered equal for simplicity.
 */
bool operator==(const EntryAttributes& lhs, const EntryAttributes& rhs);
bool operator!=(const EntryAttributes& lhs, const EntryAttributes& rhs);
bool operator==(
    const folly::Try<EntryAttributes>& lhs,
    const folly::Try<EntryAttributes>& rhs);

/**
 * Computes an initial mode_t, including permission bits, from a FileType.
 */
mode_t modeFromTreeEntryType(TreeEntryType ft);

/**
 * Converts an arbitrary mode_t to the appropriate TreeEntryType if the file
 * can be tracked by version control.  If not, returns std::nullopt.
 */
std::optional<TreeEntryType> treeEntryTypeFromMode(mode_t mode);

/**
 * Compares two optional TreeEntryType values for equality, with special
 * handling for Windows:
 * - On Windows, EXECUTABLE_FILE and REGULAR_FILE are considered equivalent for
 * comparison purposes, since Windows does not reliably distinguish executable
 * bits.
 * - On non-Windows platforms, the types are compared directly with no special
 * handling. This function ensures consistent type comparison semantics across
 * platforms.
 */
bool compareTreeEntryType(
    std::optional<TreeEntryType> lhs,
    std::optional<TreeEntryType> rhs);

class TreeEntry {
 public:
  explicit TreeEntry(
      ObjectId&& id,
      TreeEntryType type,
      bool isRestricted = false,
      std::optional<bool> hasACL = std::nullopt)
      : type_(type),
        id_(std::move(id)),
        aclRootState_(makeAclRootState(isRestricted, hasACL)) {}

  explicit TreeEntry(ObjectId&& id, TreeEntryType type, AclRootState state)
      : type_(type), id_(std::move(id)), aclRootState_(state) {}

  explicit TreeEntry(
      ObjectId&& id,
      TreeEntryType type,
      std::optional<uint64_t> size,
      std::optional<Hash20> contentSha1,
      std::optional<Hash32> contentBlake3,
      bool isRestricted = false,
      std::optional<bool> hasACL = std::nullopt)
      : type_(type),
        id_(std::move(id)),
        size_(size),
        contentSha1_(contentSha1),
        contentBlake3_(contentBlake3),
        aclRootState_(makeAclRootState(isRestricted, hasACL)) {}

  explicit TreeEntry(
      ObjectId&& id,
      TreeEntryType type,
      std::optional<uint64_t> size,
      std::optional<Hash20> contentSha1,
      std::optional<Hash32> contentBlake3,
      AclRootState state)
      : type_(type),
        id_(std::move(id)),
        size_(size),
        contentSha1_(contentSha1),
        contentBlake3_(contentBlake3),
        aclRootState_(state) {}

  const ObjectId& getObjectId() const {
    return id_;
  }

  bool isTree() const {
    return type_ == TreeEntryType::TREE;
  }

  /**
   * Returns the file type as it is stored in source control.
   */
  TreeEntryType getType() const {
    return type_;
  }

  dtype_t getDtype() const {
    switch (type_) {
      case TreeEntryType::TREE:
        return dtype_t::Dir;
      case TreeEntryType::REGULAR_FILE:
      case TreeEntryType::EXECUTABLE_FILE:
        return dtype_t::Regular;
      case TreeEntryType::SYMLINK:
        return dtype_t::Symlink;
      default:
        return dtype_t::Unknown;
    }
  }

  std::string toLogString(PathComponentPiece name) const;

  const std::optional<uint64_t>& getSize() const {
    return size_;
  }

  const std::optional<Hash20>& getContentSha1() const {
    return contentSha1_;
  }

  const std::optional<Hash32>& getContentBlake3() const {
    return contentBlake3_;
  }

  bool isRestricted() const {
    return aclRootState_ == AclRootState::RestrictedAclRoot;
  }

  std::optional<bool> hasACL() const {
    return hasACLFromAclRootState(aclRootState_);
  }

  AclRootState aclRootState() const {
    return aclRootState_;
  }

 private:
  TreeEntryType type_;
  ObjectId id_;
  std::optional<uint64_t> size_;
  std::optional<Hash20> contentSha1_;
  std::optional<Hash32> contentBlake3_;
  AclRootState aclRootState_{AclRootState::Unknown};
};

} // namespace facebook::eden
