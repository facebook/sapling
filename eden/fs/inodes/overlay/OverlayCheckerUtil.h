/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>

#include <folly/CppAttributes.h>
#include <folly/small_vector.h>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

namespace folly {
class File;
}

namespace facebook::eden::fsck {

enum class InodeType {
  Unknown,
  File,
  Dir,
  Error,
};

struct InodeInfo {
  explicit InodeInfo(InodeNumber num) : number(num), type(InodeType::Unknown) {}
  InodeInfo(InodeNumber num, InodeType t) : number(num), type(t) {}
  InodeInfo(InodeNumber num, InodeType t, std::string e)
      : number(num), type(t), errorMsg{e} {}
  InodeInfo(InodeNumber num, overlay::OverlayDir&& c)
      : number(num),
        type(InodeType::Dir),
        children(std::make_unique<overlay::OverlayDir>(std::move(c))) {}

  void addParent(InodeNumber parent, mode_t mode) {
    parents.push_back(parent);
    modeFromParent = mode;
  }

  InodeNumber number;
  InodeType type{InodeType::Error};
  std::string errorMsg;
  mode_t modeFromParent{0};
  std::unique_ptr<overlay::OverlayDir> children;
  folly::small_vector<InodeNumber, 1> parents;
};

} // namespace facebook::eden::fsck
