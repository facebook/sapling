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
#include <folly/FBString.h>
#include <folly/Range.h>
#include <folly/SharedMutex.h>
#include <boost/multi_index/composite_key.hpp>
#include <boost/multi_index/hashed_index.hpp>
#include <boost/multi_index/mem_fun.hpp>
#include <boost/multi_index/member.hpp>
#include <boost/multi_index_container.hpp>
#include <shared_mutex>
#include "eden/utils/PathFuncs.h"
#include "fuse_headers.h"

namespace facebook {
namespace eden {
namespace fusell {

/**
 * Helpers for managing name <-> inode mappings.
 */
class InodeNameManager {
 public:
  class Node {
    fuse_ino_t parentId_;
    fuse_ino_t nodeId_;
    uint64_t generation_;
    PathComponent name_;

   public:
    PathComponentPiece getName() const;
    fuse_ino_t getNodeId() const;
    fuse_ino_t getParentNodeId() const;
    uint64_t getGeneration() const;

    struct NameKey
        : public boost::multi_index::composite_key<
              Node,
              BOOST_MULTI_INDEX_MEMBER(Node, fuse_ino_t, parentId_),
              boost::multi_index::
                  const_mem_fun<Node, PathComponentPiece, &Node::getName>> {};
    using IdKey =
        boost::multi_index::member<Node, fuse_ino_t, &Node::nodeId_>;

    Node(
        fuse_ino_t parent,
        fuse_ino_t ino,
        uint64_t generation,
        PathComponentPiece name);

    void renamed(fuse_ino_t newParent, PathComponentPiece newName);
  };

 public:
  InodeNameManager();
  std::shared_ptr<Node> getNodeById(fuse_ino_t ino,
                                    bool mustExist = true) const;

  std::shared_ptr<Node>
  getNodeByName(fuse_ino_t parent, PathComponentPiece name, bool create = true);

  void rename(
      fuse_ino_t parent,
      PathComponentPiece name,
      fuse_ino_t newParent,
      PathComponentPiece newName);
  void unlink(fuse_ino_t parent, PathComponentPiece name);

  std::shared_ptr<Node> link(
      fuse_ino_t ino,
      uint64_t generation,
      fuse_ino_t newParent,
      PathComponentPiece name);

  // Returns the list of inodes that make up the path to ino
  // [root, grandParent, parent, ino] and a lock that guards
  // the set from mutation
  struct LockedNodeSet {
    std::shared_lock<folly::SharedMutex> guard;
    std::vector<std::shared_ptr<Node>> nodes;

    explicit LockedNodeSet(folly::SharedMutex& lock);
    LockedNodeSet(const LockedNodeSet&) = delete;
    LockedNodeSet& operator=(const LockedNodeSet&) = delete;
    LockedNodeSet(LockedNodeSet&&) = default;
    LockedNodeSet& operator=(LockedNodeSet&&) = default;
  };
  LockedNodeSet resolvePathAsNodes(fuse_ino_t ino) const;
  RelativePath resolvePathToNode(fuse_ino_t ino) const;
  static std::shared_ptr<InodeNameManager> get();

 private:
  mutable folly::SharedMutex lock_;
  fuse_ino_t nextNodeId_{FUSE_ROOT_ID};
  uint64_t generationCounter_{1}; // How many times nextNodeId_ has rolled

  // Tags to symbolically reference specific indices in the node map
  struct NameIndex {};
  struct IdIndex {};

  using NodeMap = boost::multi_index_container<
      std::shared_ptr<Node>,
      boost::multi_index::indexed_by<
          // Primary key is the inode number
          boost::multi_index::hashed_unique<boost::multi_index::tag<IdIndex>,
                                            Node::IdKey>,
          // Also key by name within parent.  This doesn't allow for an
          // arbitrary graph, so hard links are not possible.
          boost::multi_index::hashed_unique<boost::multi_index::tag<NameIndex>,
                                            Node::NameKey>>>;
  NodeMap map_;

  std::pair<fuse_ino_t, uint64_t> nextId();
};

}
}
}
