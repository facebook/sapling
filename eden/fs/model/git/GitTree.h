/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>

namespace facebook {
namespace eden {

class Hash;
class Tree;
class TreeEntry;

/**
 * Creates an Eden Tree from the serialized version of a Git tree object.
 * As such, the SHA-1 of the gitTreeObject should match the hash.
 */
std::unique_ptr<Tree> deserializeGitTree(
    const Hash& hash,
    const folly::IOBuf* treeData);
std::unique_ptr<Tree> deserializeGitTree(
    const Hash& hash,
    folly::ByteRange treeData);

/*
 * A class for serializing git tree objects in a streaming fashion.
 *
 * Call addEntry() with each entry in the tree, and then finalize()
 * to produce the final blob.  Note that it is the caller's responsibility to
 * properly order the calls to addEntry().
 */
class GitTreeSerializer {
 public:
  GitTreeSerializer();
  // Movable but not copiable
  GitTreeSerializer(GitTreeSerializer&&) noexcept;
  GitTreeSerializer& operator=(GitTreeSerializer&&) noexcept;
  virtual ~GitTreeSerializer();

  /**
   * Add the next entry to this tree.
   *
   * Note that the order in which entries are added is important, as this
   * affects the resulting tree hash.
   *
   * It is the callers responsibility to ensure that addEntry() is called in
   * the proper order.
   */
  void addEntry(const TreeEntry& entry);

  /**
   * Finish serializing the tree, once all entries have been added.
   *
   * Returns an IOBuf containing the serialized data.
   * addEntry() can no longer be called after calling finalize().
   */
  folly::IOBuf finalize();

 private:
  folly::IOBuf buf_;
  folly::io::Appender appender_;
};
} // namespace eden
} // namespace facebook
