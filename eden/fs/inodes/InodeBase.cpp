/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "InodeBase.h"

#include <folly/Likely.h>
#include "TreeInode.h"

using namespace folly;

namespace facebook {
namespace eden {

InodeBase::~InodeBase() {
  VLOG(5) << "inode " << this << " destroyed: " << getLogPath();
}

InodeBase::InodeBase(
    fuse_ino_t ino,
    TreeInodePtr parent,
    PathComponentPiece name)
    : ino_(ino), location_(LocationInfo(std::move(parent), name)) {
  // Inode numbers generally shouldn't be 0.
  // Older versions of glibc have bugs handling files with an inode number of 0
  DCHECK_NE(ino_, 0);
  VLOG(5) << "inode " << this << " created: " << getLogPath();
}

// See Dispatcher::getattr
folly::Future<fusell::Dispatcher::Attr> InodeBase::getattr() {
  FUSELL_NOT_IMPL();
}

// See Dispatcher::setattr
folly::Future<fusell::Dispatcher::Attr> InodeBase::setattr(
    const struct stat& /* attr */,
    int /* to_set */) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> InodeBase::setxattr(folly::StringPiece name,
                                               folly::StringPiece value,
                                               int flags) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::string> InodeBase::getxattr(folly::StringPiece name) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::vector<std::string>> InodeBase::listxattr() {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::removexattr(folly::StringPiece name) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::access(int mask) {
  FUSELL_NOT_IMPL();
}

/**
 * Helper function for getPath() and getLogPath()
 *
 * Populates the names vector with the list of PathComponents from the root
 * down to this inode.
 *
 * This method should not be called on the root inode.  The caller is
 * responsible for checking that before calling getPathHelper().
 *
 * Returns true if the the file exists at the given path, or false if the file
 * has been unlinked.
 *
 * If stopOnUnlinked is true, it breaks immediately when it finds that the file
 * has been unlinked.  The contents of the names vector are then undefined if
 * the function returns false.
 *
 * If stopOnUnlinked is false it continues building the names vector even if
 * the file is unlinked, which will then contain the path that the file used to
 * exist at.  (This path should be used only for logging purposes at that
 * point.)
 */
bool InodeBase::getPathHelper(
    std::vector<PathComponent>& names,
    bool stopOnUnlinked) const {
  TreeInodePtr parent;
  bool unlinked = false;
  {
    auto loc = location_.rlock();
    if (loc->unlinked) {
      if (stopOnUnlinked) {
        return false;
      }
      unlinked = true;
    }
    parent = loc->parent;
    // Our caller should ensure that we are not the root
    DCHECK(parent);
    names.push_back(loc->name);
  }

  while (true) {
    // Stop at the root inode.
    // We check for this based on inode number so we can stop without having to
    // acquire the root inode's location lock.  (Otherwise all path lookups
    // would have to acquire the root's lock, making it more likely to be
    // contended.)
    if (parent->ino_ == FUSE_ROOT_ID) {
      // Reverse the names vector, since we built it from bottom to top.
      std::reverse(names.begin(), names.end());
      return !unlinked;
    }

    auto loc = parent->location_.rlock();
    // In general our parent should not be unlinked if we are not unlinked,
    // which we checked above.  However, we have since released our location
    // lock, so it's possible (but unlikely) that someone unlinked us and our
    // parent directories since we checked above.
    if (UNLIKELY(loc->unlinked)) {
      if (stopOnUnlinked) {
        return false;
      }
      unlinked = true;
    }
    names.push_back(loc->name);
    parent = loc->parent;
    DCHECK(parent);
  }
}

folly::Optional<RelativePath> InodeBase::getPath() const {
  if (ino_ == FUSE_ROOT_ID) {
    return RelativePath();
  }

  std::vector<PathComponent> names;
  if (!getPathHelper(names, true)) {
    return folly::none;
  }
  return RelativePath(names);
}

std::string InodeBase::getLogPath() const {
  if (ino_ == FUSE_ROOT_ID) {
    // We use "<root>" here instead of the empty string to make log messages
    // more understandable.  The empty string would likely be confusing, as it
    // would appear if the file name were missing.
    return "<root>";
  }

  std::vector<PathComponent> names;
  bool unlinked = !getPathHelper(names, false);
  auto path = RelativePath(names);
  if (unlinked) {
    return folly::to<std::string>("<deleted:", path.stringPiece(), ">");
  }
  // TODO: We should probably adjust the PathFuncs code to use std::string
  // instead of fbstring.  For FB builds, std::string is the fbstring
  // implementation.  For external builds, with gcc 5+, std::string is very
  // similar to fbstring anyway.
  //
  // return std::move(path).value();
  return path.stringPiece().str();
}

void InodeBase::markUnlinked() {
  VLOG(5) << "inode " << this << " unlinked: " << getLogPath();
  auto loc = location_.wlock();
  DCHECK(!loc->unlinked);
  loc->unlinked = true;
}

void InodeBase::updateLocation(
    TreeInodePtr newParent,
    PathComponentPiece newName) {
  VLOG(5) << "inode " << this << " renamed: " << getLogPath() << " --> "
          << newParent->getLogPath() << " / \"" << newName << "\"";
  auto loc = location_.wlock();
  DCHECK(!loc->unlinked);
  loc->parent = newParent;
  loc->name = newName.copy();
}
}
}
