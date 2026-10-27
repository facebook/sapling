/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/fs/store/WalkDetector.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h"

namespace facebook::eden {

/**
 * The WalkDetector of one mount of a SaplingBackingStore, backed by the Rust
 * walk detector and prefetcher for that mount. Every report passes the Rust
 * store along, which is how the detector follows the store's configuration.
 */
class SaplingWalkDetector final : public WalkDetector {
 public:
  SaplingWalkDetector(
      std::shared_ptr<sapling::BackingStore> store,
      rust::Box<sapling::WalkDetector> detector);

  void witnessFileRead(
      const ObjectId& id,
      ObjectFetchContext::Origin origin,
      OptionalProcessId pid,
      std::optional<ObjectFetchContext::DirEntryCounts> parentCounts) override;

  void witnessTreeRead(
      const ObjectId& id,
      const Tree& tree,
      ObjectFetchContext::Origin origin,
      OptionalProcessId pid) override;

  void witnessDirectorySize(
      RelativePathPiece path,
      ObjectFetchContext::DirEntryCounts counts) override;

  void workingCopyParentHint(const RootId& parent) override;

 private:
  std::shared_ptr<sapling::BackingStore> store_;
  rust::Box<sapling::WalkDetector> detector_;
};

} // namespace facebook::eden
