/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/TreeEntry.h"

#include <ostream>

#include <folly/Conv.h>
#include <folly/Range.h>

namespace facebook {
namespace eden {

std::string TreeEntry::toLogString() const {
  char fileTypeChar = '?';
  switch (fileType_) {
    case FileType::DIRECTORY:
      fileTypeChar = 'd';
      break;
    case FileType::REGULAR_FILE:
      fileTypeChar = 'f';
      break;
    case FileType::EXECUTABLE_FILE:
      fileTypeChar = 'x';
      break;
    case FileType::SYMLINK:
      fileTypeChar = 'l';
      break;
  }

  return folly::to<std::string>(
      "(", name_, ", ", hash_.toString(), ", ", fileTypeChar, ")");
}

std::ostream& operator<<(std::ostream& os, FileType type) {
  switch (type) {
    case FileType::DIRECTORY:
      return os << "DIRECTORY";
    case FileType::REGULAR_FILE:
      return os << "REGULAR_FILE";
    case FileType::EXECUTABLE_FILE:
      return os << "EXECUTABLE_FILE";
    case FileType::SYMLINK:
      return os << "SYMLINK";
  }

  return os << "FileType::" << int(type);
}

std::ostream& operator<<(std::ostream& os, TreeEntryType type) {
  switch (type) {
    case TreeEntryType::TREE:
      return os << "TREE";
    case TreeEntryType::BLOB:
      return os << "BLOB";
  }

  return os << "TreeEntryType::" << int(type);
}

bool operator==(const TreeEntry& entry1, const TreeEntry& entry2) {
  return (entry1.getHash() == entry2.getHash()) &&
      (entry1.getFileType() == entry2.getFileType()) &&
      (entry1.getName() == entry2.getName());
}

bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2) {
  return !(entry1 == entry2);
}
} // namespace eden
} // namespace facebook
