/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/memcatalog/MemInodeCatalog.h"
#include <algorithm>
#include <utility>

#include "eden/fs/inodes/sqlitecatalog/WindowsFsck.h"

namespace facebook::eden {

// Initial Inode ID is root ID + 1
constexpr auto kInitialNodeId = kRootNodeId.getRawValue() + 1;

/**
 * `Overlay` only uses this method to control cleanup, which in this case is
 * unneeded, so return false to bypass.
 */
bool MemInodeCatalog::initialized() const {
  return false;
}

std::vector<InodeNumber> MemInodeCatalog::getAllParentInodeNumbers() {
  auto store = store_.rlock();
  std::vector<InodeNumber> result;
  std::transform(
      store->begin(),
      store->end(),
      std::back_inserter(result),
      [](const auto& kv) { return kv.first; });
  return result;
}

std::optional<InodeNumber> MemInodeCatalog::initOverlay(
    bool /* createIfNonExisting */,
    bool /* bypassLockFile */) {
  nextInode_ = kInitialNodeId;
  return InodeNumber(nextInode_.load());
}

/**
 * Because `initialized` always returns false there is nothing to do on close.
 */
void MemInodeCatalog::close(std::optional<InodeNumber> /*nextInodeNumber*/) {}

std::optional<overlay::OverlayDir> MemInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  auto store = store_.rlock();
  auto itr = store->find(inodeNumber);
  return itr != store->end()
      ? std::make_optional<overlay::OverlayDir>(itr->second)
      : std::nullopt;
}

std::optional<overlay::OverlayDir> MemInodeCatalog::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  auto store = store_.wlock();
  auto itr = store->find(inodeNumber);
  if (itr != store->end()) {
    auto overlayDir = std::move(itr->second);
    store->erase(itr);
    return overlayDir;
  } else {
    return std::nullopt;
  }
}

void MemInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  auto store = store_.wlock();
  store->insert_or_assign(inodeNumber, std::move(odir));
}

void MemInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  auto store = store_.wlock();
  auto itr = store->find(inodeNumber);
  if (itr == store->end() || !itr->second.entries_ref()->empty()) {
    throw NonEmptyError("cannot delete non-empty directory");
  }

  store->erase(itr);
}

bool MemInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  auto store = store_.rlock();
  auto itr = store->find(inodeNumber);
  return itr != store->end();
}

void MemInodeCatalog::addChild(
    InodeNumber parent,
    PathComponentPiece name,
    overlay::OverlayEntry entry) {
  auto store = store_.wlock();
  auto itr = store->find(parent);
  if (itr != store->end()) {
    itr->second.entries()->emplace(name.asString(), std::move(entry));
  } else {
    overlay::OverlayDir odir;
    odir.entries()->emplace(name.asString(), std::move(entry));
    store->emplace(parent, std::move(odir));
  }
}

void MemInodeCatalog::removeChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  auto store = store_.wlock();
  auto itr = store->find(parent);
  if (itr != store->end()) {
    auto entries = itr->second.entries_ref();
    entries->erase(childName.asString());
  }
}

bool MemInodeCatalog::hasChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  auto store = store_.rlock();
  auto itr = store->find(parent);
  if (itr == store->end() || itr->second.entries_ref()->empty()) {
    return false;
  }

  auto entries = itr->second.entries_ref();
  return entries->count(childName.asString()) != 0;
}

void MemInodeCatalog::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName) {
  auto store = store_.wlock();

  // Check if dst directory exits
  auto dstOdir = store->find(dst);
  if (dstOdir != store->end()) {
    // Check if dst named child exists
    auto dstEntries = dstOdir->second.entries_ref();
    auto dstChild = dstEntries->find(dstName.asString());
    if (dstChild != dstEntries->end()) {
      // Check if dst child has children
      auto childIno = InodeNumber(dstChild->second.get_inodeNumber());
      auto childOdir = store->find(childIno);
      if (childOdir != store->end()) {
        throw NonEmptyError("cannot overwrite non-empty directory");
      }
    }
  }

  // Check if src directory exits
  auto srcOdir = store->find(src);
  if (srcOdir != store->end()) {
    // Check if src named child exists
    auto srcEntries = srcOdir->second.entries_ref();
    auto srcChild = srcEntries->find(srcName.asString());
    if (srcChild != srcEntries->end()) {
      if (dstOdir == store->end()) {
        // Create dst and include src child in entries
        overlay::OverlayDir odir;
        odir.entries_ref()->emplace(dstName.asString(), srcChild->second);
        store->emplace(dst, std::move(odir));
      } else {
        // Use existing dst
        auto dstEntries = dstOdir->second.entries_ref();
        dstEntries[dstName.asString()] = srcChild->second;
      }
      srcEntries->erase(srcChild);
    }
  }
}

InodeNumber MemInodeCatalog::nextInodeNumber() {
  return InodeNumber{nextInode_.fetch_add(1, std::memory_order_acq_rel)};
}

std::optional<fsck::InodeInfo> MemInodeCatalog::loadInodeInfo(
    FOLLY_MAYBE_UNUSED InodeNumber number) {
  return std::nullopt;
}

InodeNumber MemInodeCatalog::scanLocalChanges(
    FOLLY_MAYBE_UNUSED std::shared_ptr<const EdenConfig> config,
    FOLLY_MAYBE_UNUSED AbsolutePathPiece mountPath,
    FOLLY_MAYBE_UNUSED bool windowsSymlinksEnabled,
    FOLLY_MAYBE_UNUSED InodeCatalog::LookupCallback& callback) {
#ifdef _WIN32
  windowsFsckScanLocalChanges(
      config,
      *this,
      InodeCatalogType::InMemory,
      mountPath,
      windowsSymlinksEnabled,
      callback);
#endif
  return InodeNumber{nextInode_.load()};
}
} // namespace facebook::eden
