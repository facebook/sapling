/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GitTree.h"
#include <folly/Format.h>
#include <folly/String.h>
#include <array>
#include <cstdio>
#include <cstring>
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

using folly::IOBuf;
using std::invalid_argument;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

enum GitModeMask {
  DIRECTORY = 040000,
  GIT_LINK = 0160000,
  REGULAR_EXECUTABLE_FILE = 0100755,
  REGULAR_FILE = 0100644,
  SYMLINK = 0120000,
};

std::unique_ptr<Tree> deserializeGitTree(
    const Hash& hash,
    const IOBuf* treeData) {
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
  vector<TreeEntry> entries;
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

    // Extract the hash.
    Hash::Storage hashBytes;
    cursor.pull(hashBytes.data(), hashBytes.size());

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
      throw std::domain_error(folly::sformat(
          "Gitlinks are not currently supported: {:o} in object {}",
          static_cast<int>(mode),
          hash.toString()));
    } else {
      throw invalid_argument(folly::sformat(
          "Unrecognized mode: {:o} in object {}",
          static_cast<int>(mode),
          hash.toString()));
    }

    entries.emplace_back(Hash(hashBytes), std::move(name), fileType);
  }

  return std::make_unique<Tree>(std::move(entries), hash);
}

// Convenience wrapper which accepts a ByteRange
std::unique_ptr<Tree> deserializeGitTree(
    const Hash& hash,
    folly::ByteRange treeData) {
  IOBuf buf(IOBuf::WRAP_BUFFER, treeData);
  return deserializeGitTree(hash, &buf);
}

enum size_t {
  // Initially allocate 4kb of data for the tree buffer.
  INITIAL_TREE_BUF_SIZE = 4096,
  // Grow by 4kb at a time if we need more space
  TREE_BUF_GROW_SIZE = 4096,
  // Leave 32 bytes of headroom for the "tree+<size>" prefix
  TREE_PREFIX_HEADROOM = 32
};

GitTreeSerializer::GitTreeSerializer()
    : buf_(IOBuf::CREATE, INITIAL_TREE_BUF_SIZE),
      appender_(&buf_, TREE_BUF_GROW_SIZE) {
  // Leave a bit of headroom, so we can stuff in the "tree" and size
  // prefix afterwards.
  buf_.advance(TREE_PREFIX_HEADROOM);
}

GitTreeSerializer::GitTreeSerializer(GitTreeSerializer&& other) noexcept
    : buf_(std::move(other.buf_)), appender_(&buf_, TREE_BUF_GROW_SIZE) {
  // Reset other.appender_ too, just for safety's sake,
  // even though the caller shouldn't use it any more.
  other.appender_ = folly::io::Appender(&other.buf_, TREE_BUF_GROW_SIZE);
}

GitTreeSerializer& GitTreeSerializer::operator=(
    GitTreeSerializer&& other) noexcept {
  // Moving the IOBuf invalidates the Appender pointing at it
  // so we can't simply move other.appender_, we have to initialize our
  // Appender from scratch.
  buf_ = std::move(other.buf_);
  appender_ = folly::io::Appender(&buf_, TREE_BUF_GROW_SIZE);
  // Reset other.appender_ too, just for safety's sake, even though the
  // caller shouldn't use it any more.
  other.appender_ = folly::io::Appender(&other.buf_, TREE_BUF_GROW_SIZE);
  return *this;
}

GitTreeSerializer::~GitTreeSerializer() {}

void GitTreeSerializer::addEntry(const TreeEntry& entry) {
  // Note: We don't do any sorting of the entries.  We simply serialize them in
  // the order given to us by the caller.  It is up to the caller to ensure
  // that the entries are sorted in the correct order.  (The sorting order does
  // affect the final tree hash.)

  mode_t mode = 0;
  switch (entry.getType()) {
    case TreeEntryType::EXECUTABLE_FILE:
      mode = GitModeMask::REGULAR_EXECUTABLE_FILE;
      break;
    case TreeEntryType::REGULAR_FILE:
      mode = GitModeMask::REGULAR_FILE;
      break;
    case TreeEntryType::TREE:
      mode = GitModeMask::DIRECTORY;
      break;
    case TreeEntryType::SYMLINK:
      mode = GitModeMask::SYMLINK;
      break;
  }
  if (!mode) {
    throw std::runtime_error(folly::to<string>(
        "unsupported entry type ",
        static_cast<int>(entry.getType()),
        " for ",
        entry.getName().stringPiece()));
  }

  appender_.printf("%o ", mode);
  appender_.push(entry.getName().stringPiece());
  appender_.write<uint8_t>(0);
  appender_.push(entry.getHash().getBytes());
}

folly::IOBuf GitTreeSerializer::finalize() {
  // Add the header onto the tree buffer
  std::array<char, TREE_PREFIX_HEADROOM> header;
  auto headerLength = snprintf(
      header.data(), header.size(), "tree %zu", buf_.computeChainDataLength());
  if (headerLength < 0 || static_cast<size_t>(headerLength) >= header.size()) {
    // This shouldn't ever happen in practice
    throw std::runtime_error("error formatting tree header");
  }
  headerLength += 1; // Include the terminating NUL byte

  CHECK_GE(buf_.headroom(), headerLength);
  buf_.prepend(headerLength);
  memcpy(buf_.writableData(), header.data(), headerLength);

  return std::move(buf_);
}
} // namespace eden
} // namespace facebook
