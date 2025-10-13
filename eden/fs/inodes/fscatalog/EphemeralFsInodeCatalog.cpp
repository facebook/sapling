/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/fscatalog/EphemeralFsInodeCatalog.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"

#include <boost/filesystem.hpp>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/io/IOBuf.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/utils/NotImplemented.h"

#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook::eden {

using apache::thrift::CompactSerializer;
using folly::ByteRange;
using folly::fbvector;
using folly::File;
using folly::IOBuf;
using folly::MutableStringPiece;
using folly::StringPiece;
using folly::literals::string_piece_literals::operator""_sp;
using std::optional;
using std::string;

bool EphemeralFsInodeCatalog::initialized() const {
  return core_->initialized();
}

std::optional<InodeNumber> EphemeralFsInodeCatalog::initOverlay(
    bool /*createIfNonExisting*/,
    bool bypassLockFile) {
  // Manually just pass createIfNonExisting=true to initialize() since we only
  // support fresh overlays with this InodeCatalog type.
  bool overlayCreated =
      core_->initialize(/*createIfNonExisting*/ true, bypassLockFile);

  if (!overlayCreated) {
    folly::throwSystemError(
        "EphemeralFsInodeCatalog only supports fresh overlays but a pre-existing overlay was found");
  }

  return InodeNumber{kRootNodeId.get() + 1};
}

void EphemeralFsInodeCatalog::close(
    std::optional<InodeNumber> /*inodeNumber*/) {
  // At the time of writing, this just closes some files in the
  // FsFileContentStore. Overlays using this InodeCatalog type cannot be
  // re-open, but closing a couple of files is cheap enough that we might as
  // well do it for completeness sake.
  core_->close();
}

std::vector<InodeNumber> EphemeralFsInodeCatalog::getAllParentInodeNumbers() {
  auto store = store_.rlock();
  std::vector<InodeNumber> result;
  std::transform(
      store->begin(),
      store->end(),
      std::back_inserter(result),
      [](const auto& kv) { return kv.first; });
  return result;
}

std::optional<overlay::OverlayDir> EphemeralFsInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  auto store = store_.rlock();
  auto itr = store->find(inodeNumber);
  return itr != store->end()
      ? std::make_optional<overlay::OverlayDir>(itr->second)
      : std::nullopt;
}

std::optional<overlay::OverlayDir>
EphemeralFsInodeCatalog::loadAndRemoveOverlayDir(InodeNumber inodeNumber) {
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

void EphemeralFsInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  auto store = store_.wlock();
  store->insert_or_assign(inodeNumber, std::move(odir));
}

void EphemeralFsInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  auto store = store_.wlock();
  auto itr = store->find(inodeNumber);
  if (itr == store->end() || !itr->second.entries()->empty()) {
    throw NonEmptyError("cannot delete non-empty directory");
  }

  store->erase(itr);
}

bool EphemeralFsInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  auto store = store_.rlock();
  auto itr = store->find(inodeNumber);
  return itr != store->end();
}

void EphemeralFsInodeCatalog::addChild(
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

bool EphemeralFsInodeCatalog::removeChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  auto store = store_.wlock();
  auto itr = store->find(parent);
  if (itr != store->end()) {
    auto entries = itr->second.entries();
    entries->erase(childName.asString());
    return true;
  }
  // The child does not exist
  return false;
}

bool EphemeralFsInodeCatalog::hasChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  auto store = store_.rlock();
  auto itr = store->find(parent);
  if (itr == store->end() || itr->second.entries()->empty()) {
    return false;
  }

  auto entries = itr->second.entries();
  return entries->count(childName.asString()) != 0;
}

void EphemeralFsInodeCatalog::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName) {
  auto store = store_.wlock();

  // Check if dst directory exits
  auto dstOdir = store->find(dst);
  if (dstOdir != store->end()) {
    // Check if dst named child exists
    auto dstEntries = dstOdir->second.entries();
    auto dstChild = dstEntries->find(dstName.asString());
    if (dstChild != dstEntries->end()) {
      // Check if dst child has children
      auto childIno =
          InodeNumber(folly::copy(dstChild->second.inodeNumber().value()));
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
    auto srcEntries = srcOdir->second.entries();
    auto srcChild = srcEntries->find(srcName.asString());
    if (srcChild != srcEntries->end()) {
      if (dstOdir == store->end()) {
        // Create dst and include src child in entries
        overlay::OverlayDir odir;
        odir.entries()->emplace(dstName.asString(), srcChild->second);
        store->emplace(dst, std::move(odir));
      } else {
        // Use existing dst
        auto dstEntries = dstOdir->second.entries();
        dstEntries[dstName.asString()] = srcChild->second;
      }
      srcEntries->erase(srcChild);
    }
  }
}

std::optional<fsck::InodeInfo> EphemeralFsInodeCatalog::loadInodeInfo(
    InodeNumber /*number*/) {
  // These InodeCatalogs don't support fsck since they are only ever expected to
  // be used for fresh overlays.
  NOT_IMPLEMENTED();
}

} // namespace facebook::eden

#endif
