/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

#include <folly/Range.h>
#include <folly/testing/TestUtil.h>

#include "eden/fs/inodes/InodeNumber.h"

namespace facebook::eden {

class FsFileContentStore;
class FsInodeCatalog;

/**
 * Upper bound on the size of a synthesized overlay file.
 *
 * Both read paths slurp the whole file into a std::string before parsing it,
 * and the Thrift compact protocol sizes its containers from the buffer it is
 * handed, so capping the file caps the allocation a single input can provoke.
 */
inline constexpr size_t kMaxFuzzedOverlayFileSize = 8192;

/**
 * Inode numbers the harness writes overlay files for. Every value maps into
 * one of the 256 shard directories that initialize() already created, so no
 * iteration has to create a directory.
 */
inline constexpr uint64_t kFuzzInodeBase = 100;
inline constexpr uint64_t kFuzzInodeCount = 256;

enum class OverlayLoadOutcome {
  /// The read path returned a parsed directory.
  Parsed,
  /// The read path found no overlay file for the inode.
  Missing,
  /// The read path rejected the file by throwing.
  Rejected,
};

struct OverlayDirLoadResult {
  OverlayLoadOutcome outcome{OverlayLoadOutcome::Rejected};
  /// Number of entries in the parsed directory. Only meaningful for Parsed.
  size_t entryCount{0};
};

struct OverlayEntriesLoadResult {
  OverlayLoadOutcome outcome{OverlayLoadOutcome::Rejected};
  /// The map size the parser read out of the compact-protocol header.
  size_t declaredCount{0};
  /// How many entries the visitor was actually handed.
  size_t visitedCount{0};
};

/**
 * A real, on-disk Legacy overlay that fuzz iterations write single files into.
 *
 * kInodeCatalogTypeDefault is InodeCatalogType::Legacy on Linux and macOS, so
 * FsInodeCatalog over FsFileContentStore is the catalog production reads back
 * after every restart.
 *
 * The overlay directory is created once per process rather than once per
 * iteration: FsFileContentStore::initialize() creates 256 shard directories
 * plus 256 sharded_tmp directories, and paying ~520 mkdir syscalls per
 * iteration would dominate the parse being fuzzed. Nothing on the read path
 * mutates the overlay outside the single file under test, and each iteration
 * unlinks that file before returning, so iterations stay independent.
 */
class OverlayFileFuzzFixture {
 public:
  OverlayFileFuzzFixture();
  ~OverlayFileFuzzFixture();

  OverlayFileFuzzFixture(const OverlayFileFuzzFixture&) = delete;
  OverlayFileFuzzFixture& operator=(const OverlayFileFuzzFixture&) = delete;
  OverlayFileFuzzFixture(OverlayFileFuzzFixture&&) = delete;
  OverlayFileFuzzFixture& operator=(OverlayFileFuzzFixture&&) = delete;

  /**
   * The process-wide fixture. Constructed on first use and torn down during
   * static destruction, which is what removes the temporary directory.
   */
  static OverlayFileFuzzFixture& global();

  static InodeNumber pickInode(uint8_t selector);

  /**
   * Write `fileContents` as the overlay file for `ino`, run
   * FsInodeCatalog::loadOverlayDir (header validation followed by
   * CompactSerializer::deserialize<overlay::OverlayDir>), then unlink the file.
   *
   * Contents longer than kMaxFuzzedOverlayFileSize are truncated. Exceptions
   * from the read path are reported as Rejected rather than propagated, so an
   * expected rejection is not mistaken for a harness failure.
   */
  OverlayDirLoadResult loadOverlayDir(
      InodeNumber ino,
      folly::ByteRange fileContents);

  /**
   * As loadOverlayDir, but drives FsInodeCatalog::loadOverlayEntries, which
   * shares the same header validation and then hand-decodes the compact
   * protocol instead of using the generated deserializer.
   */
  OverlayEntriesLoadResult loadOverlayEntries(
      InodeNumber ino,
      folly::ByteRange fileContents);

  /**
   * The 64 header bytes that saveOverlayDir writes, captured from a real
   * write through the catalog rather than reconstructed, so the harness
   * cannot drift from the production format.
   */
  const std::string& validDirHeader() const {
    return validDirHeader_;
  }

 private:
  void writeOverlayFile(InodeNumber ino, folly::ByteRange fileContents);
  std::string captureValidDirHeader();

  folly::test::TemporaryDirectory tmpDir_;
  std::unique_ptr<FsFileContentStore> store_;
  std::unique_ptr<FsInodeCatalog> catalog_;
  std::string validDirHeader_;
};

} // namespace facebook::eden
