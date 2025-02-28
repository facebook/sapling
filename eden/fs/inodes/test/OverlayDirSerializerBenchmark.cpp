/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/stop_watch.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <algorithm>
#include <cstdlib>
#include <functional>
#include <random>

#include "eden/common/telemetry/NullStructuredLogger.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/InodeCatalogType.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/telemetry/EdenStats.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;

DEFINE_string(overlayPath, "", "Directory where the test overlay is created");
DEFINE_bool(
    copy,
    false,
    "Set this parameter to test copying instead of serializing");
DEFINE_string(
    overlayType,
    kDefaultInodeCatalogType == InodeCatalogType::Sqlite ? "Sqlite" : "Legacy",
    "Type of overlay to be used. Defaults: Windows - Sqlite; Linux|macOS - Legacy");

namespace {

constexpr uint64_t kOverlayItems = 50;
constexpr uint64_t kIterations = 500000;

void copyOverlayDirectory(
    std::shared_ptr<Overlay> overlay,
    InodeCatalog* inodeCatalog,
    const DirContents& contents) {
  // Test copying the OverlayDir directly
  printf("Overlay data written. Starting benchmark for copies...\n");

  std::vector<folly::Function<void()>> fns;

  folly::stop_watch<> copyTimer;

  for (uint64_t i = 1; i <= kIterations; i++) {
    auto inodeNumber = overlay->allocateInodeNumber();

    fns.emplace_back(
        [inodeCatalog,
         inodeNumber,
         odir = overlay->serializeOverlayDir(inodeNumber, contents)]() mutable {
          inodeCatalog->saveOverlayDir(inodeNumber, std::move(odir));
        });
  }

  for (auto& fn : fns) {
    fn();
  }

  auto copyElapsed = copyTimer.elapsed();

  printf(
      "Total elapsed time for copying %" SCNu64 " entries: %.2f s\n",
      kIterations,
      std::chrono::duration_cast<std::chrono::duration<double>>(copyElapsed)
          .count());

  printf(
      "Average time per copy call: %.2f us\n",
      static_cast<double>(
          std::chrono::duration_cast<std::chrono::duration<double, std::micro>>(
              copyElapsed / kIterations)
              .count()));
}

void serializeOverlayDirectory(
    std::shared_ptr<Overlay> overlay,
    InodeCatalog* inodeCatalog,
    const DirContents& contents) {
  // Test serialize the OverlayDir into a std::string
  printf("Overlay data written. Starting benchmark for serializing...\n");

  std::vector<folly::Function<void()>> fns;

  folly::stop_watch<> serializeTimer;

  for (uint64_t i = 1; i <= kIterations; i++) {
    auto inodeNumber = overlay->allocateInodeNumber();
    overlay::OverlayDir odir =
        overlay->serializeOverlayDir(inodeNumber, contents);

    auto serializedOverlayDir =
        apache::thrift::CompactSerializer::serialize<std::string>(odir);

    fns.emplace_back([inodeCatalog,
                      inodeNumber,
                      serializedOverlayDir =
                          std::move(serializedOverlayDir)]() mutable {
      auto deserializedOverlayDir =
          apache::thrift::CompactSerializer::deserialize<overlay::OverlayDir>(
              serializedOverlayDir);
      inodeCatalog->saveOverlayDir(
          inodeNumber, std::move(deserializedOverlayDir));
    });
  }

  for (auto& fn : fns) {
    fn();
  }

  auto serializeElapsed = serializeTimer.elapsed();

  printf(
      "Total elapsed time for serializing %" SCNu64 " entries: %.2f s\n",
      kIterations,
      std::chrono::duration_cast<std::chrono::duration<double>>(
          serializeElapsed)
          .count());

  printf(
      "Average time per serialize call: %.2f us\n",
      static_cast<double>(
          std::chrono::duration_cast<std::chrono::duration<double, std::micro>>(
              serializeElapsed / kIterations)
              .count()));
}

void benchmarkOverlayDirSerialization(
    AbsolutePathPiece overlayPath,
    InodeCatalogType overlayType) {
  // A large mount will contain 500,000 trees. If they're all loaded, they
  // will all be written into the overlay. This benchmark simulates that
  // workload and measures how long it takes.
  //
  // overlayPath is parameterized to measure on different filesystem types.
  printf("Creating Overlay...\n");

  auto overlay = Overlay::create(
      overlayPath,
      kPathMapDefaultCaseSensitive,
      overlayType,
      kDefaultInodeCatalogOptions,
      std::make_shared<NullStructuredLogger>(),
      makeRefPtr<EdenStats>(),
      true,
      *EdenConfig::createTestEdenConfig());
  printf("Initializing Overlay...\n");

  overlay->initialize({EdenConfig::createTestEdenConfig()}).get();

  printf("Overlay initialized. Writing overlay data...\n");

  std::vector<char> chars{'0', '1', '2', '3', '4', '5', '6', '7', '8',
                          '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h',
                          'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
                          'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z'};

  std::default_random_engine rng(std::random_device{}());
  std::uniform_int_distribution<> dist(0, chars.size() - 1);

  DirContents contents(kPathMapDefaultCaseSensitive);

  for (uint64_t i = 1; i <= kOverlayItems; i++) {
    std::string str(20, 0);
    std::generate_n(str.begin(), 20, [&]() { return chars[dist(rng)]; });
    folly::StringPiece sp{str};

    contents.emplace(
        PathComponent{sp},
        S_IFREG | 0644,
        overlay->allocateInodeNumber(),
        ObjectId{folly::ByteRange{sp}});
  }

  InodeCatalog* inodeCatalog = overlay->getRawInodeCatalog();

  if (FLAGS_copy) {
    copyOverlayDirectory(overlay, inodeCatalog, contents);
  } else {
    serializeOverlayDirectory(overlay, inodeCatalog, contents);
  }

  folly::stop_watch<> closeTimer;

  overlay->close();

  auto closeElapsed = closeTimer.elapsed();

  printf(
      "Total elapsed time to close Overlay: %.2f s\n",
      std::chrono::duration_cast<std::chrono::duration<double>>(closeElapsed)
          .count());
}

} // namespace

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);

  if (FLAGS_overlayPath.empty()) {
    fprintf(stderr, "error: overlayPath is required\n");
    return 1;
  }

  auto overlayPath = normalizeBestEffort(FLAGS_overlayPath.c_str());
  auto overlayType = inodeCatalogTypeFromString(FLAGS_overlayType);
  benchmarkOverlayDirSerialization(overlayPath, overlayType.value());

  return 0;
}
