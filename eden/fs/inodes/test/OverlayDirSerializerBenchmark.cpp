/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/stop_watch.h>
#include <stdlib.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <algorithm>
#include <functional>
#include <random>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/IOverlay.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;

DEFINE_string(overlayPath, "", "Directory where the test overlay is created");
DEFINE_bool(
    copy,
    false,
    "Set this parameter to test copying instead of serializing");

namespace {

constexpr uint64_t kOverlayItems = 50;
constexpr uint64_t kIterations = 500000;

void copyOverlayDirectory(
    std::shared_ptr<Overlay> overlay,
    IOverlay* backingOverlay,
    const DirContents& contents) {
  // Test copying the OverlayDir directly
  printf("Overlay data written. Starting benchmark for copies...\n");

  std::vector<folly::Function<void()>> fns;

  folly::stop_watch<> copyTimer;

  for (uint64_t i = 1; i <= kIterations; i++) {
    auto inodeNumber = overlay->allocateInodeNumber();

    fns.emplace_back(
        [backingOverlay,
         inodeNumber,
         odir = overlay->serializeOverlayDir(inodeNumber, contents)]() mutable {
          backingOverlay->saveOverlayDir(inodeNumber, std::move(odir));
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
    IOverlay* backingOverlay,
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

    fns.emplace_back([backingOverlay,
                      inodeNumber,
                      serializedOverlayDir =
                          std::move(serializedOverlayDir)]() mutable {
      auto deserializedOverlayDir =
          apache::thrift::CompactSerializer::deserialize<overlay::OverlayDir>(
              serializedOverlayDir);
      backingOverlay->saveOverlayDir(
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

void benchmarkOverlayDirSerialization(AbsolutePathPiece overlayPath) {
  // A large mount will contain 500,000 trees. If they're all loaded, they
  // will all be written into the overlay. This benchmark simulates that
  // workload and measures how long it takes.
  //
  // overlayPath is parameterized to measure on different filesystem types.
  printf("Creating Overlay...\n");

  auto overlay = Overlay::create(
      overlayPath,
      kPathMapDefaultCaseSensitive,
      kDefaultOverlayType,
      std::make_shared<NullStructuredLogger>(),
      *EdenConfig::createTestEdenConfig());
  printf("Initalizing Overlay...\n");

  overlay->initialize().get();

  printf("Overlay initalized. Writing overlay data...\n");

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

  IOverlay* backingOverlay = overlay->getRawBackingOverlay();

  if (FLAGS_copy) {
    copyOverlayDirectory(overlay, backingOverlay, contents);
  } else {
    serializeOverlayDirectory(overlay, backingOverlay, contents);
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
  benchmarkOverlayDirSerialization(overlayPath);

  return 0;
}
