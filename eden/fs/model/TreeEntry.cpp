/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/model/TreeEntry.h"

#include <folly/Conv.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <sys/stat.h>
#include <ostream>

namespace facebook {
namespace eden {

#ifndef _WIN32
mode_t modeFromTreeEntryType(TreeEntryType ft) {
  switch (ft) {
    case TreeEntryType::TREE:
      return S_IFDIR | 0755;
    case TreeEntryType::REGULAR_FILE:
      return S_IFREG | 0644;
    case TreeEntryType::EXECUTABLE_FILE:
      return S_IFREG | 0755;
    case TreeEntryType::SYMLINK:
      return S_IFLNK | 0755;
  }
  XLOG(FATAL) << "illegal file type " << static_cast<int>(ft);
}

std::optional<TreeEntryType> treeEntryTypeFromMode(mode_t mode) {
  if (S_ISREG(mode)) {
    return mode & S_IXUSR ? TreeEntryType::EXECUTABLE_FILE
                          : TreeEntryType::REGULAR_FILE;
  } else if (S_ISLNK(mode)) {
    return TreeEntryType::SYMLINK;
  } else if (S_ISDIR(mode)) {
    return TreeEntryType::TREE;
  } else {
    return std::nullopt;
  }
}
#endif

std::string TreeEntry::toLogString() const {
  char fileTypeChar = '?';
  switch (type_) {
    case TreeEntryType::TREE:
      fileTypeChar = 'd';
      break;
    case TreeEntryType::REGULAR_FILE:
      fileTypeChar = 'f';
      break;
    case TreeEntryType::EXECUTABLE_FILE:
      fileTypeChar = 'x';
      break;
    case TreeEntryType::SYMLINK:
      fileTypeChar = 'l';
      break;
  }

  return folly::to<std::string>(
      "(", name_, ", ", hash_.toString(), ", ", fileTypeChar, ")");
}

std::ostream& operator<<(std::ostream& os, TreeEntryType type) {
  switch (type) {
    case TreeEntryType::TREE:
      return os << "TREE";
    case TreeEntryType::REGULAR_FILE:
      return os << "REGULAR_FILE";
    case TreeEntryType::EXECUTABLE_FILE:
      return os << "EXECUTABLE_FILE";
    case TreeEntryType::SYMLINK:
      return os << "SYMLINK";
  }

  return os << "TreeEntryType::" << int(type);
}

bool operator==(const TreeEntry& entry1, const TreeEntry& entry2) {
  return (entry1.getHash() == entry2.getHash()) &&
      (entry1.getType() == entry2.getType()) &&
      (entry1.getName() == entry2.getName());
}

bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2) {
  return !(entry1 == entry2);
}
} // namespace eden
} // namespace facebook
