/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "InodeNameManager.h"
#include <folly/Exception.h>
#include <folly/FBVector.h>
#include <folly/String.h>
#include <shared_mutex>

using namespace folly;

DEFINE_int32(namemap_reserve,
             1000000,
             "pre-size name hash table for this many entries");

namespace facebook {
namespace eden {
namespace fusell {

InodeNameManager::Node::Node(
    fuse_ino_t parent,
    fuse_ino_t ino,
    uint64_t generation,
    PathComponentPiece name)
    : parentId_(parent), nodeId_(ino), generation_(generation), name_(name) {}

PathComponentPiece InodeNameManager::Node::getName() const {
  return name_.piece();
}
fuse_ino_t InodeNameManager::Node::getNodeId() const { return nodeId_; }
fuse_ino_t InodeNameManager::Node::getParentNodeId() const { return parentId_; }
uint64_t InodeNameManager::Node::getGeneration() const { return generation_; }

InodeNameManager::InodeNameManager() {
  map_.reserve(FLAGS_namemap_reserve);
}

std::shared_ptr<InodeNameManager::Node> InodeNameManager::getNodeById(
    fuse_ino_t ino, bool mustExist) const {
  std::shared_lock<SharedMutex> g(lock_);
  auto &idIndex = map_.get<IdIndex>();
  auto it = idIndex.find(ino);
  if (it == idIndex.end()) {
    if (mustExist) {
      throwSystemErrorExplicit(ENOENT);
    }
    return nullptr;
  }
  return *it;
}

std::pair<fuse_ino_t, uint64_t> InodeNameManager::nextId() {
  auto &idIndex = map_.get<IdIndex>();
  while (true) {
    ++nextNodeId_;

    if (nextNodeId_ == 0) {
      // Check for rolling over.  We never hand out ino=0
      ++generationCounter_;
      continue;
    }

    // Ensure that we're not colliding
    auto it = idIndex.find(nextNodeId_);
    if (it == idIndex.end()) {
      return std::make_pair(nextNodeId_, generationCounter_);
    }
  }
}

std::shared_ptr<InodeNameManager::Node> InodeNameManager::getNodeByName(
    fuse_ino_t parent,
    PathComponentPiece name,
    bool create) {
  {
    std::shared_lock<SharedMutex> g(lock_);
    auto& nameIndex = map_.get<NameIndex>();
    auto it = nameIndex.find(std::make_tuple(parent, name));
    if (it != nameIndex.end()) {
      return *it;
    }

    if (!create) {
      return nullptr;
    }
  }

  {
    std::unique_lock<SharedMutex> g(lock_);
    auto& nameIndex = map_.get<NameIndex>();

    // May have lost a race while upgrading to a unique lock
    auto it = nameIndex.find(std::make_tuple(parent, name));
    if (it != nameIndex.end()) {
      return *it;
    }

    auto idpair = nextId();
    auto node = std::make_shared<Node>(parent, idpair.first, idpair.second, name);
    map_.insert(node);

    return node;
  }
}

void InodeNameManager::unlink(fuse_ino_t parent, PathComponentPiece name) {
  std::unique_lock<SharedMutex> g(lock_);
  auto& nameIndex = map_.get<NameIndex>();
  auto it = nameIndex.find(std::make_tuple(parent, name));
  if (it != nameIndex.end()) {
    nameIndex.erase(it);
  }
}

std::shared_ptr<InodeNameManager::Node> InodeNameManager::link(
    fuse_ino_t ino,
    uint64_t generation,
    fuse_ino_t newParent,
    PathComponentPiece name) {
  throwSystemErrorExplicit(
      EACCES,
      "sorry, there's an ambiguity with resolving paths when we have multiple "
      "parents, need to adjust the accessors before you can safely use this");
  std::unique_lock<SharedMutex> g(lock_);
  auto& nameIndex = map_.get<NameIndex>();
  auto it = nameIndex.find(std::make_tuple(newParent, name));
  if (it != nameIndex.end()) {
    throwSystemErrorExplicit(EEXIST);
  }

  auto node = std::make_shared<Node>(newParent, ino, generation, name);
  map_.insert(node);

  return node;
}

void InodeNameManager::Node::renamed(
    fuse_ino_t newParent,
    PathComponentPiece newName) {
  parentId_ = newParent;
  name_ = newName.copy();
}

void InodeNameManager::rename(
    fuse_ino_t parent,
    PathComponentPiece name,
    fuse_ino_t newParent,
    PathComponentPiece newName) {
  std::unique_lock<SharedMutex> g(lock_);
  auto& nameIndex = map_.get<NameIndex>();
  auto it = nameIndex.find(std::make_tuple(parent, name));
  if (it == nameIndex.end()) {
    throwSystemErrorExplicit(ENOENT);
  }
  auto node = *it;

  // Remove from old
  nameIndex.erase(it);

  // Now we'll re-insert with the new parent info
  node->renamed(newParent, newName);
  map_.insert(node);
}

InodeNameManager::LockedNodeSet::LockedNodeSet(SharedMutex& lock)
    : guard(lock) {}

InodeNameManager::LockedNodeSet InodeNameManager::resolvePathAsNodes(
    fuse_ino_t ino) const {
  LockedNodeSet nodeset(lock_);

  auto &idIndex = map_.get<IdIndex>();
  while (true) {
    auto it = idIndex.find(ino);
    if (it == idIndex.end()) {
      throwSystemErrorExplicit(ENOENT);
    }
    auto node = *it;
    nodeset.nodes.insert(nodeset.nodes.begin(), node);
    if (ino == FUSE_ROOT_ID) {
      return nodeset;
    }
    ino = node->getParentNodeId();
  }
}

RelativePath InodeNameManager::resolvePathToNode(fuse_ino_t ino) const {
  folly::fbvector<PathComponentPiece> bits;

  std::shared_lock<SharedMutex> g(lock_);
  auto &idIndex = map_.get<IdIndex>();

  while (ino != FUSE_ROOT_ID) {
    auto it = idIndex.find(ino);
    if (it == idIndex.end()) {
      throwSystemErrorExplicit(ENOENT);
    }
    auto node = *it;
    bits.insert(bits.begin(), node->getName());

    ino = node->getParentNodeId();
  }

  return RelativePath(bits);
}

}
}
}
