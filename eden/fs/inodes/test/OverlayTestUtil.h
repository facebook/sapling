/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <iomanip>
#include <memory>
#include <sstream>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/ErrorLogger.h"

namespace facebook::eden {

void debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode,
    folly::StringPiece path,
    std::ostringstream& out);

inline std::string debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode) {
  std::ostringstream out;
  debugDumpOverlayInodes(overlay, rootInode, "/", out);
  return out.str();
}

inline std::shared_ptr<EdenFsEventsLogger> makeTestEdenFsEventsLogger() {
  return std::make_shared<EdenFsEventsLogger>(nullptr);
}

/**
 * Create a no-op ErrorLogger for use in unit tests.
 * Scribe is null so log() returns immediately.
 */
inline ErrorLogger makeTestErrorLogger() {
  return ErrorLogger{nullptr, {}, nullptr};
}

// Friend of Overlay so tests can drive the private WAL compaction path
// directly and inject a deterministic RNG (the production default uses
// folly::Random::rand32()).
class OverlayTestHelper {
 public:
  static void maybeCompactWal(
      Overlay& overlay,
      InodeNumber parent,
      const DirContents& content,
      uint64_t walFileSizeBytes = 0) {
    overlay.maybeCompactWal(parent, content, walFileSizeBytes);
  }

  static void setWalCompactionRng(
      Overlay& overlay,
      std::function<uint32_t()> rng) {
    overlay.walCompactionRng_ = std::move(rng);
  }
};

} // namespace facebook::eden
