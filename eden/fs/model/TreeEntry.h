/*
 *  Copyright (c) 2016-present, Facebook, Inc.
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
#include "eden/fs/utils/PathFuncs.h"

#include <folly/String.h>
#include <folly/experimental/logging/xlog.h>
#include <sys/stat.h>
#include <iosfwd>

namespace facebook {
namespace eden {

class Hash;

enum class TreeEntryType : uint8_t {
  BLOB,
  TREE,
};

enum class FileType : uint8_t {
  DIRECTORY,
  REGULAR_FILE,
  EXECUTABLE_FILE,
  SYMLINK,
};

class TreeEntry {
 public:
  explicit TreeEntry(
      const Hash& hash,
      folly::StringPiece name,
      FileType fileType)
      : fileType_(fileType), hash_(hash), name_(PathComponentPiece(name)) {}

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

  mode_t getMode() const {
    switch (fileType_) {
      case FileType::DIRECTORY:
        return S_IFDIR | 0755;
      case FileType::REGULAR_FILE:
        return S_IFREG | 0644;
      case FileType::EXECUTABLE_FILE:
        return S_IFREG | 0755;
      case FileType::SYMLINK:
        return S_IFLNK | 0755;
    }
    XLOG(FATAL) << "illegal file type " << int(fileType_);
  }

  std::string toLogString() const;

 private:
  FileType fileType_;
  Hash hash_;
  PathComponent name_;
};

std::ostream& operator<<(std::ostream& os, FileType type);
std::ostream& operator<<(std::ostream& os, TreeEntryType type);
bool operator==(const TreeEntry& entry1, const TreeEntry& entry2);
bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2);
} // namespace eden
} // namespace facebook
