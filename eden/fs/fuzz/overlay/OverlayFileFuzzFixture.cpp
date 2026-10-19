/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuzz/overlay/OverlayFileFuzzFixture.h"

#include <algorithm>
#include <exception>
#include <string>
#include <utility>
#include <vector>

#include <folly/FileUtil.h>
#include <folly/ScopeGuard.h>

#include "eden/common/testharness/TempFile.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

namespace facebook::eden {

namespace {
/// Inode used only to capture the header bytes saveOverlayDir emits. Kept
/// outside the range pickInode draws from so a fuzz iteration cannot race it.
constexpr InodeNumber kHeaderProbeInode{2};
} // namespace

OverlayFileFuzzFixture::OverlayFileFuzzFixture()
    : tmpDir_{makeTempDir("eden_overlay_fuzz")} {
  store_ = std::make_unique<FsFileContentStore>(
      canonicalPath(tmpDir_.path().string()));
  store_->initialize(/*createIfNonExisting=*/true);
  catalog_ = std::make_unique<FsInodeCatalog>(store_.get());
  validDirHeader_ = captureValidDirHeader();
}

OverlayFileFuzzFixture::~OverlayFileFuzzFixture() {
  catalog_->close(std::nullopt);
  catalog_.reset();
  store_.reset();
}

OverlayFileFuzzFixture& OverlayFileFuzzFixture::global() {
  static OverlayFileFuzzFixture fixture;
  return fixture;
}

InodeNumber OverlayFileFuzzFixture::pickInode(uint8_t selector) {
  return InodeNumber{kFuzzInodeBase + (selector % kFuzzInodeCount)};
}

std::string OverlayFileFuzzFixture::captureValidDirHeader() {
  catalog_->saveOverlayDir(
      kHeaderProbeInode, overlay::OverlayDir{}, /*crashSafe=*/false);
  SCOPE_EXIT {
    catalog_->removeOverlayDir(kHeaderProbeInode);
  };

  std::string contents;
  auto path = store_->getAbsoluteFilePath(kHeaderProbeInode);
  if (!folly::readFile(path.c_str(), contents)) {
    throw std::runtime_error("failed to read back probe overlay file");
  }
  if (contents.size() < FsFileContentStore::kHeaderLength) {
    throw std::runtime_error("probe overlay file is shorter than its header");
  }
  contents.resize(FsFileContentStore::kHeaderLength);
  return contents;
}

void OverlayFileFuzzFixture::writeOverlayFile(
    InodeNumber ino,
    folly::ByteRange fileContents) {
  auto path = store_->getAbsoluteFilePath(ino);
  if (!folly::writeFile(fileContents, path.c_str())) {
    throw std::runtime_error("failed to write fuzzed overlay file");
  }
}

OverlayDirLoadResult OverlayFileFuzzFixture::loadOverlayDir(
    InodeNumber ino,
    folly::ByteRange fileContents) {
  fileContents = fileContents.subpiece(
      0, std::min(fileContents.size(), kMaxFuzzedOverlayFileSize));
  writeOverlayFile(ino, fileContents);
  SCOPE_EXIT {
    catalog_->removeOverlayDir(ino);
  };

  try {
    auto dir = catalog_->loadOverlayDir(ino);
    if (!dir.has_value()) {
      return OverlayDirLoadResult{OverlayLoadOutcome::Missing};
    }
    return OverlayDirLoadResult{
        OverlayLoadOutcome::Parsed, dir->entries()->size()};
  } catch (const std::exception&) {
    return OverlayDirLoadResult{OverlayLoadOutcome::Rejected};
  }
}

OverlayEntriesLoadResult OverlayFileFuzzFixture::loadOverlayEntries(
    InodeNumber ino,
    folly::ByteRange fileContents) {
  fileContents = fileContents.subpiece(
      0, std::min(fileContents.size(), kMaxFuzzedOverlayFileSize));
  writeOverlayFile(ino, fileContents);
  SCOPE_EXIT {
    catalog_->removeOverlayDir(ino);
  };

  OverlayEntriesLoadResult result{OverlayLoadOutcome::Missing};
  std::vector<std::pair<std::string, overlay::OverlayEntry>> entries;
  try {
    bool found = catalog_->loadOverlayEntries(
        ino, [&](size_t count, InodeCatalog::OverlayEntryIterator iterate) {
          result.declaredCount = count;
          // Overlay::loadOverlayDir reserves from this count before any entry
          // is decoded (Overlay.cpp), so the harness does too: a count that
          // stopped being bounded against the file size upstream has to show
          // up here as an allocation, not just as a loop bound.
          entries.reserve(count);
          iterate(
              [&](const std::string& name, const overlay::OverlayEntry& entry) {
                entries.emplace_back(name, entry);
                ++result.visitedCount;
              });
        });
    result.outcome =
        found ? OverlayLoadOutcome::Parsed : OverlayLoadOutcome::Missing;
  } catch (const std::exception&) {
    result.outcome = OverlayLoadOutcome::Rejected;
  }
  return result;
}

} // namespace facebook::eden
