/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "Hash.h"
#include "TreeEntry.h"
#include "eden/utils/PathFuncs.h"

#include <folly/String.h>

namespace facebook {
namespace eden {

class Hash;

enum class TreeEntryType {
  BLOB,
  TREE,
};

enum class FileType {
  // The values for these enum entries correspond to the ones we would expect to
  // find in <sys/stat.h>. We hardcode them just in case the values in the
  // users's <sys/stat.h> are different. Note that Git performs a similar check:
  // https://github.com/git/git/blob/v2.7.4/configure.ac#L912-L917
  DIRECTORY = 0040,
  REGULAR_FILE = 0100,
  SYMLINK = 0120,
};

class TreeEntry {
 public:
  explicit TreeEntry(
      const Hash& hash,
      folly::StringPiece name,
      FileType fileType,
      uint8_t ownerPermissions)
      : fileType_(fileType),
        ownerPermissions_(ownerPermissions),
        hash_(hash),
        name_(PathComponentPiece(name)) {}

  const Hash& getHash() const {
    return hash_;
  }

  const PathComponent& getName() const {
    return name_;
  }

  TreeEntryType getType() const {
    return fileType_ == FileType::DIRECTORY ? TreeEntryType::TREE
                                            : TreeEntryType::BLOB;
  }

  FileType getFileType() const {
    return fileType_;
  }

  uint8_t getOwnerPermissions() const {
    return ownerPermissions_;
  }

  mode_t getMode() const {
    mode_t mode = static_cast<mode_t>(fileType_) << 9;
    // We should always honor the explicit owner permissions.
    mode |= ownerPermissions_ << 6;

    // We propagate the 'r' and 'x' values for the owner to group and other.
    mode |= (ownerPermissions_ & 0b101) << 3;
    mode |= (ownerPermissions_ & 0b101);
    return mode;
  }

  /**
   * Extract the owner permissions from a mode_t value.
   *
   * This can be used to construct the ownerPermissions parameter needed for
   * the TreeEntry constructor.
   */
  static uint8_t modeToOwnerPermissions(mode_t mode) {
    return (mode >> 6) & 0b111;
  }

 private:
  FileType fileType_;
  uint8_t ownerPermissions_;
  Hash hash_;
  PathComponent name_;
};
}
}
