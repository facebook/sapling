/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/sl/SaplingWalkDetector.h"

#include <string_view>

#include "eden/fs/model/Tree.h"
#include "eden/fs/store/sl/SaplingObjectId.h"

namespace facebook::eden {

namespace {

rust::Slice<const uint8_t> bytes(std::string_view view) {
  return rust::Slice<const uint8_t>{
      reinterpret_cast<const uint8_t*>(view.data()), view.size()};
}

} // namespace

SaplingWalkDetector::SaplingWalkDetector(
    std::shared_ptr<sapling::BackingStore> store,
    rust::Box<sapling::WalkDetector> detector)
    : store_{std::move(store)}, detector_{std::move(detector)} {}

void SaplingWalkDetector::witnessFileRead(
    const ObjectId& id,
    ObjectFetchContext::Origin origin,
    OptionalProcessId pid,
    std::optional<ObjectFetchContext::DirEntryCounts> parentCounts) {
  auto path = SlOidView{id}.path().view();
  if (path.empty()) {
    return;
  }
  sapling::sapling_walk_detector_witness_file_read(
      *store_,
      *detector_,
      bytes(path),
      origin == ObjectFetchContext::FromDiskCache,
      pid.valueOrZero().get(),
      parentCounts.has_value(),
      parentCounts ? parentCounts->files : 0,
      parentCounts ? parentCounts->dirs : 0);
}

void SaplingWalkDetector::witnessTreeRead(
    const ObjectId& id,
    const Tree& tree,
    ObjectFetchContext::Origin origin,
    OptionalProcessId pid) {
  auto path = SlOidView{id}.path().view();
  if (path.empty()) {
    return;
  }
  size_t files = 0;
  size_t dirs = 0;
  for (const auto& entry : tree) {
    if (entry.second.isTree()) {
      ++dirs;
    } else {
      ++files;
    }
  }
  sapling::sapling_walk_detector_witness_dir_read(
      *store_,
      *detector_,
      bytes(path),
      files,
      dirs,
      origin == ObjectFetchContext::FromDiskCache,
      pid.valueOrZero().get());
}

void SaplingWalkDetector::witnessDirectorySize(
    RelativePathPiece path,
    ObjectFetchContext::DirEntryCounts counts) {
  sapling::sapling_walk_detector_witness_dir_size(
      *store_, *detector_, bytes(path.view()), counts.files, counts.dirs);
}

void SaplingWalkDetector::workingCopyParentHint(const RootId& parent) {
  sapling::sapling_walk_detector_set_parent_hint(
      *store_, *detector_, parent.value());
}

} // namespace facebook::eden
