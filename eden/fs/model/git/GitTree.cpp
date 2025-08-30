/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/git/GitTree.h"
#include <fmt/format.h>
#include <folly/io/Cursor.h>
#include <cstring>
#include "eden/common/utils/Throw.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

using folly::IOBuf;
using std::invalid_argument;
using std::vector;

namespace facebook::eden {

enum GitModeMask {
  DIRECTORY = 040000,
  GIT_LINK = 0160000,
  REGULAR_EXECUTABLE_FILE = 0100755,
  REGULAR_FILE = 0100644,
  SYMLINK = 0120000,
};

TreePtr deserializeGitTree(const ObjectId& id, const IOBuf* treeData) {
  folly::io::Cursor cursor(treeData);

  // Find the end of the header and extract the size.
  if (cursor.readFixedString(5) != "tree ") {
    throw invalid_argument("Contents did not start with expected header.");
  }

  // 25 characters is long enough to represent any legitimate length
  size_t maxSizeLength = 25;
  auto sizeStr = cursor.readTerminatedString('\0', maxSizeLength);
  auto contentSize = folly::to<unsigned int>(sizeStr);
  if (contentSize != cursor.length()) {
    throw invalid_argument("Size in header should match contents");
  }

  // Scan the data and populate entries, as appropriate.
  Tree::container entries{kPathMapDefaultCaseSensitive};
  while (!cursor.isAtEnd()) {
    // Extract the mode.
    // This should only be 6 or 7 characters.
    // Stop scanning if we haven't seen a space in 10 characters
    size_t maxModeLength = 10;
    auto modeStr = cursor.readTerminatedString(' ', maxModeLength);
    size_t modeEndIndex;
    auto mode = std::stoi(modeStr, &modeEndIndex, /* base */ 8);
    if (modeEndIndex != modeStr.size()) {
      throw invalid_argument("Did not parse expected number of octal chars.");
    }

    // Extract the name.
    auto name = cursor.readTerminatedString();

    // Extract the id.
    Hash20::Storage idBytes;
    cursor.pull(idBytes.data(), idBytes.size());

    // Determine the individual fields from the mode.

    TreeEntryType fileType;
    if (mode == GitModeMask::DIRECTORY) {
      fileType = TreeEntryType::TREE;
    } else if (mode == GitModeMask::REGULAR_FILE) {
      fileType = TreeEntryType::REGULAR_FILE;
    } else if (mode == GitModeMask::REGULAR_EXECUTABLE_FILE) {
      fileType = TreeEntryType::EXECUTABLE_FILE;
    } else if (mode == GitModeMask::SYMLINK) {
      fileType = TreeEntryType::SYMLINK;
    } else if (mode == GitModeMask::GIT_LINK) {
      throwf<std::domain_error>(
          "Gitlinks are not currently supported: {:o} in object {}", mode, id);
    } else {
      throw invalid_argument(
          fmt::format("Unrecognized mode: {:o} in object {}", mode, id));
    }

    auto pathName = PathComponentPiece{name};
    entries.emplace(pathName, ObjectId(idBytes), fileType);
  }

  return std::make_shared<TreePtr::element_type>(std::move(entries), id);
}

// Convenience wrapper which accepts a ByteRange
TreePtr deserializeGitTree(const ObjectId& id, folly::ByteRange treeData) {
  IOBuf buf(IOBuf::WRAP_BUFFER, treeData);
  return deserializeGitTree(id, &buf);
}

} // namespace facebook::eden
