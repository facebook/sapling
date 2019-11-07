/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/init/Init.h>
#include <folly/stop_watch.h>
#include <gflags/gflags.h>
#include <stdlib.h>
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/Overlay.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;

DEFINE_string(overlayPath, "", "Directory where the test overlay is created");

namespace {

void benchmarkOverlayTreeWrites(AbsolutePathPiece overlayPath) {
  // A large mount will contain 500,000 trees. If they're all loaded, they
  // will all be written into the overlay. This benchmark simulates that
  // workload and measures how long it takes.
  //
  // overlayPath is parameterized to measure on different filesystem types.

  auto overlay = Overlay::create(overlayPath);
  overlay->initialize().get();

  Hash hash1{folly::ByteRange{"abcdabcdabcdabcdabcd"_sp}};
  Hash hash2{folly::ByteRange{"01234012340123401234"_sp}};

  DirContents contents;
  contents.emplace(
      PathComponent{"one"},
      S_IFREG | 0644,
      overlay->allocateInodeNumber(),
      hash1);
  contents.emplace(
      PathComponent{"two"},
      S_IFDIR | 0755,
      overlay->allocateInodeNumber(),
      hash2);

  uint64_t N = 500000;

  folly::stop_watch<> timer;

  for (uint64_t i = 1; i <= N; i++) {
    auto ino = overlay->allocateInodeNumber();
    overlay->saveOverlayDir(ino, contents);
  }

  auto elapsed = timer.elapsed();

  printf(
      "Total elapsed time for %" SCNu64 " entries: %.2f s\n",
      N,
      std::chrono::duration_cast<std::chrono::duration<double>>(elapsed)
          .count());

  // Normally, I prefer to use minimum, but the cost of writing into the
  // overlay increases as the overlay grows, as xfs especially updates its
  // btrees.
  //
  // That reason, plus the reason that we want a fixed N for comparable results
  // is why this benchmark doesn't use folly Benchmark.
  printf(
      "Average time per call: %.2f us\n",
      static_cast<double>(
          std::chrono::duration_cast<std::chrono::duration<double, std::micro>>(
              elapsed / N)
              .count()));
}

} // namespace

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);

  if (FLAGS_overlayPath.empty()) {
    fprintf(stderr, "error: overlayPath is required\n");
    return 1;
  }

  auto overlayPath = normalizeBestEffort(FLAGS_overlayPath.c_str());
  benchmarkOverlayTreeWrites(overlayPath);

  return 0;
}
