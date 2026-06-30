/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

namespace facebook::eden {

struct AclState {
  std::optional<bool> ancestorUnderAcl;
  std::optional<bool> hasACL;
};

inline std::optional<bool> mergeAncestorAclState(
    std::optional<bool> ancestorUnderAcl,
    std::optional<bool> hasACL) {
  if (ancestorUnderAcl == true || hasACL == true) {
    return true;
  }
  if (ancestorUnderAcl == false && hasACL == false) {
    return false;
  }
  return std::nullopt;
}

inline AclState adjustRootAclState(
    bool isRoot,
    std::optional<bool> ancestorUnderAcl,
    std::optional<bool> hasACL) {
  if (!isRoot || ancestorUnderAcl == true) {
    return {ancestorUnderAcl, hasACL};
  }

  if (hasACL != true) {
    hasACL = false;
  }
  return {false, hasACL};
}

} // namespace facebook::eden
