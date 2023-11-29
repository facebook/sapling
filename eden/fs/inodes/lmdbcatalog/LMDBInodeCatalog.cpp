/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/lmdbcatalog/LMDBInodeCatalog.h"

#include <folly/Range.h>
#include <folly/io/Cursor.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/lmdbcatalog/LMDBFileContentStore.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/NotImplemented.h"

namespace facebook::eden {

std::optional<InodeNumber> LMDBInodeCatalog::initOverlay(
    bool createIfNonExisting,
    bool bypassLockFile) {
  core_->initialize(createIfNonExisting, bypassLockFile);
  return core_->store_.loadCounters();
}

InodeNumber LMDBInodeCatalog::nextInodeNumber() {
  return core_->store_.nextInodeNumber();
}

void LMDBInodeCatalog::maintenance() {
  core_->store_.maintenance();
}

void LMDBInodeCatalog::close(std::optional<InodeNumber> /*nextInodeNumber*/) {
  core_->close();
}

bool LMDBInodeCatalog::initialized() const {
  return core_->initialized();
}

std::vector<InodeNumber> LMDBInodeCatalog::getAllParentInodeNumbers() {
  return core_->store_.getAllParentInodeNumbers();
}

std::optional<overlay::OverlayDir> LMDBInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  return core_->store_.loadTree(inodeNumber);
}

std::optional<overlay::OverlayDir> LMDBInodeCatalog::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  return core_->store_.loadAndRemoveTree(inodeNumber);
}

void LMDBInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  auto deserializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(
          std::move(odir));
  return core_->store_.saveTree(inodeNumber, std::move(deserializedOverlayDir));
}

void LMDBInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    std::string&& odir) {
  return core_->store_.saveTree(inodeNumber, std::move(odir));
}

void LMDBInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  core_->store_.removeTree(inodeNumber);
}

bool LMDBInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  return core_->store_.hasTree(inodeNumber);
}

std::optional<fsck::InodeInfo> LMDBInodeCatalog::loadInodeInfo(
    InodeNumber /*number*/) {
  NOT_IMPLEMENTED();
}

InodeNumber LMDBInodeCatalog::scanLocalChanges(
    std::shared_ptr<const EdenConfig> /*config*/,
    AbsolutePathPiece /*mountPath*/,
    bool /*windowsSymlinksEnabled*/,
    FOLLY_MAYBE_UNUSED InodeCatalog::LookupCallback& /*callback*/) {
  NOT_IMPLEMENTED();
}
} // namespace facebook::eden
