/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/BenchmarkUtil.h>
#include <folly/init/Init.h>
#include <folly/stop_watch.h>
#include <gflags/gflags.h>
#include <cstdlib>
#include <random>
#include <thread>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/InodeCatalogType.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/telemetry/EdenStats.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;

DEFINE_string(overlayPath, "", "Directory where the test overlay is created");
DEFINE_string(
    overlayType,
    kDefaultInodeCatalogType == InodeCatalogType::Sqlite ? "Sqlite" : "Legacy",
    "Type of overlay to be used. Defaults: Windows - Sqlite; Linux|macOS - Legacy");
DEFINE_uint64(numDirs, 100000, "Number of directories to save/load");
DEFINE_uint32(threads, 1, "Number of threads for parallel save/load");
DEFINE_uint32(
    dirSize,
    0,
    "Fixed number of entries per directory. "
    "0 means random (1-10000, exponential distribution favoring small dirs)");
DEFINE_bool(
    directSerialize,
    false,
    "Use direct serialization (skip intermediate std::map)");
DEFINE_bool(
    directFileWrites,
    false,
    "Write overlay files directly without temp+rename for non-materialized dirs");

namespace {

// Build a DirContents with the given number of entries.
DirContents buildDirContents(
    std::shared_ptr<Overlay>& overlay,
    size_t numEntries,
    std::default_random_engine& rng) {
  std::uniform_int_distribution<> charDist(0, 35);
  constexpr char chars[] = "0123456789abcdefghijklmnopqrstuvwxyz";

  DirContents contents(kPathMapDefaultCaseSensitive);

  for (size_t i = 0; i < numEntries; ++i) {
    size_t nameLen = 10 + (i % 11);
    std::string name(nameLen, 'x');
    for (auto& c : name) {
      c = chars[charDist(rng)];
    }
    name += fmt::format("_{}", i);

    auto ino = overlay->allocateInodeNumber();
    if (i % 3 == 0) {
      // Materialized entry (no hash)
      contents.emplace(PathComponentPiece{name}, S_IFREG | 0644, ino);
    } else {
      auto hashStr = fmt::format("{:040x}", i);
      contents.emplace(
          PathComponentPiece{name},
          S_IFREG | 0644,
          ino,
          ObjectId{folly::ByteRange{folly::StringPiece{hashStr}}});
    }
  }

  return contents;
}

// Returns a random directory size from 1 to 10000, biased toward smaller sizes
// using an exponential distribution.
size_t randomDirSize(std::default_random_engine& rng) {
  std::exponential_distribution<> dist(0.06);
  auto val = static_cast<size_t>(dist(rng)) + 1;
  return std::min(val, size_t{10000});
}

void benchmarkOverlay(
    AbsolutePathPiece overlayPath,
    InodeCatalogType overlayType) {
  auto N = FLAGS_numDirs;
  auto numThreads = FLAGS_threads;

  printf(
      "Config: dirs=%" SCNu64
      ", threads=%u, dirSize=%s, directSerialize=%s, directFileWrites=%s\n",
      N,
      numThreads,
      FLAGS_dirSize > 0 ? std::to_string(FLAGS_dirSize).c_str() : "random",
      FLAGS_directSerialize ? "true" : "false",
      FLAGS_directFileWrites ? "true" : "false");

  auto edenConfig = EdenConfig::createTestEdenConfig();
  if (FLAGS_directSerialize) {
    edenConfig->overlayDirectSerialization.setValue(
        true, ConfigSourceType::CommandLine);
  }
  if (FLAGS_directFileWrites) {
    edenConfig->overlayDirectFileWrites.setValue(
        true, ConfigSourceType::CommandLine);
  }

  printf("Creating Overlay...\n");

  auto overlay = Overlay::create(
      overlayPath,
      kPathMapDefaultCaseSensitive,
      overlayType,
      kDefaultInodeCatalogOptions,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      *edenConfig);

  overlay->initialize(std::make_shared<ReloadableConfig>(std::move(edenConfig)))
      .get();

  // Pre-build directories.
  printf("Building %zu directories...\n", size_t(N));
  std::default_random_engine rng(12345);
  std::vector<DirContents> dirs;
  dirs.reserve(N);
  size_t totalEntries = 0;
  for (uint64_t i = 0; i < N; ++i) {
    auto size = FLAGS_dirSize > 0 ? FLAGS_dirSize : randomDirSize(rng);
    totalEntries += size;
    dirs.push_back(buildDirContents(overlay, size, rng));
  }
  printf(
      "Built %" SCNu64 " directories (avg %.1f entries, %zu total entries)\n",
      N,
      static_cast<double>(totalEntries) / N,
      totalEntries);

  // --- Save benchmark ---
  // 90% of directories are non-materialized (matching source control),
  // 10% are materialized (user modifications). The direct-file-writes
  // optimization only applies to non-materialized directories.
  std::vector<InodeNumber> savedInodes(N);
  std::vector<bool> isMaterialized(N);
  for (uint64_t i = 0; i < N; ++i) {
    savedInodes[i] = overlay->allocateInodeNumber();
    isMaterialized[i] = (i % 10 == 0);
  }

  printf("Saving...\n");
  folly::stop_watch<> saveTimer;

  if (numThreads <= 1) {
    for (uint64_t i = 0; i < N; ++i) {
      overlay->saveOverlayDir(savedInodes[i], dirs[i], isMaterialized[i]);
    }
  } else {
    std::vector<std::thread> threads;
    threads.reserve(numThreads);
    uint64_t chunkSize = (N + numThreads - 1) / numThreads;
    for (uint32_t t = 0; t < numThreads; ++t) {
      uint64_t start = t * chunkSize;
      uint64_t end = std::min(start + chunkSize, N);
      threads.emplace_back([&, start, end]() {
        for (uint64_t i = start; i < end; ++i) {
          overlay->saveOverlayDir(savedInodes[i], dirs[i], isMaterialized[i]);
        }
      });
    }
    for (auto& thread : threads) {
      thread.join();
    }
  }

  auto saveElapsed = saveTimer.elapsed();
  printf(
      "Save: %.2f s total, %.2f us/dir\n",
      std::chrono::duration_cast<std::chrono::duration<double>>(saveElapsed)
          .count(),
      static_cast<double>(
          std::chrono::duration_cast<std::chrono::duration<double, std::micro>>(
              saveElapsed / N)
              .count()));

  // --- Load benchmark ---
  printf("Loading...\n");
  folly::stop_watch<> loadTimer;

  if (numThreads <= 1) {
    for (uint64_t i = 0; i < N; ++i) {
      auto result = overlay->loadOverlayDir(savedInodes[i]);
      folly::doNotOptimizeAway(result);
    }
  } else {
    std::vector<std::thread> threads;
    threads.reserve(numThreads);
    uint64_t chunkSize = (N + numThreads - 1) / numThreads;
    for (uint32_t t = 0; t < numThreads; ++t) {
      uint64_t start = t * chunkSize;
      uint64_t end = std::min(start + chunkSize, N);
      threads.emplace_back([&, start, end]() {
        for (uint64_t i = start; i < end; ++i) {
          auto result = overlay->loadOverlayDir(savedInodes[i]);
          folly::doNotOptimizeAway(result);
        }
      });
    }
    for (auto& thread : threads) {
      thread.join();
    }
  }

  auto loadElapsed = loadTimer.elapsed();
  printf(
      "Load: %.2f s total, %.2f us/dir\n",
      std::chrono::duration_cast<std::chrono::duration<double>>(loadElapsed)
          .count(),
      static_cast<double>(
          std::chrono::duration_cast<std::chrono::duration<double, std::micro>>(
              loadElapsed / N)
              .count()));

  folly::stop_watch<> closeTimer;
  overlay->close();
  auto closeElapsed = closeTimer.elapsed();
  printf(
      "Close: %.2f s\n",
      std::chrono::duration_cast<std::chrono::duration<double>>(closeElapsed)
          .count());
}

} // namespace

int main(int argc, char* argv[]) {
  const folly::Init init(&argc, &argv);

  if (FLAGS_overlayPath.empty()) {
    fprintf(stderr, "error: overlayPath is required\n");
    return 1;
  }

  auto overlayPath = normalizeBestEffort(FLAGS_overlayPath.c_str());
  auto overlayType = inodeCatalogTypeFromString(FLAGS_overlayType);
  benchmarkOverlay(overlayPath, overlayType.value());

  return 0;
}
