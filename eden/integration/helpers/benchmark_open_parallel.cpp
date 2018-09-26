/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/Likely.h>
#include <folly/init/Init.h>
#include <folly/synchronization/Baton.h>
#include <gflags/gflags.h>
#include <inttypes.h>
#include <string.h>
#include <unistd.h>
#include <limits>
#include <mutex>
#include <system_error>
#include <thread>

DEFINE_uint64(threads, 1, "The number of concurrent open/close threads");
DEFINE_uint64(iterations, 100000, "Number of open/close iterations per thread");

namespace {
uint64_t gettime() {
  timespec ts;
  // CLOCK_MONOTONIC_RAW might be better but these benchmarks are short and
  // reading CLOCK_MONOTONIC takes 20 ns and CLOCK_MONOTONIC_RAW takes 130 ns.
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ts.tv_sec * 1000000000 + ts.tv_nsec;
}

class StatAccumulator {
 public:
  void add(uint64_t value) {
    minimum_ = std::min(minimum_, value);
    total_ += value;
  }

  void combine(StatAccumulator other) {
    minimum_ = std::min(minimum_, other.minimum_);
    total_ += other.total_;
  }

  uint64_t getMinimum() const {
    return minimum_;
  }

  uint64_t getAverage(uint64_t count) const {
    return total_ / count;
  }

 private:
  uint64_t minimum_{std::numeric_limits<uint64_t>::max()};
  uint64_t total_{0};
};

uint64_t measure_clock_overhead() {
  constexpr int N = 100000;
  constexpr bool kUseMinimum = true;

  StatAccumulator accum;

  uint64_t last = gettime();
  for (int i = 0; i < N; ++i) {
    uint64_t next = gettime();
    uint64_t elapsed = next - last;
    accum.add(elapsed);
    last = next;
  }

  return kUseMinimum ? accum.getMinimum() : accum.getAverage(N);
}
} // namespace

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  if (argc <= 1) {
    fprintf(
        stderr,
        "Specify a list of filenames on the command line. They will be opened "
        "in sequence.\n");
    return 1;
  }

  uint64_t clock_overhead = measure_clock_overhead();
  printf("Clock overhead measured at %" PRIu64 " ns\n", clock_overhead);

  // Prefetch every specified file.
  for (int i = 1; i < argc; ++i) {
    auto filename = argv[i];
    int fd = ::open(filename, O_RDONLY);
    if (UNLIKELY(-1 == fd)) {
      perror(folly::to<std::string>("Failed to open '", filename, "'").c_str());
      return 1;
    }
    ::close(fd);
  }

  folly::Baton<> baton;

  std::mutex result_mutex;
  StatAccumulator combined_open;
  StatAccumulator combined_close;

  auto thread = [&] {
    StatAccumulator open_accum;
    StatAccumulator close_accum;
    int file_index = 1;

    baton.wait();

    for (uint64_t i = 0; i < FLAGS_iterations; ++i) {
      const char* filename = argv[file_index];

      uint64_t start_time = gettime();
      int fd = ::open(filename, O_RDONLY);
      uint64_t after_open = gettime();
      if (UNLIKELY(-1 == fd)) {
        folly::throwSystemError("Failed to open '", filename, "'");
      }
      ::close(fd);
      uint64_t after_close = gettime();

      if (++file_index >= argc) {
        file_index = 1;
      }

      open_accum.add(after_open - start_time);
      close_accum.add(after_close - after_open);
    }

    std::lock_guard guard{result_mutex};
    combined_open.combine(open_accum);
    combined_close.combine(close_accum);
  };

  std::vector<std::thread> threads;
  threads.reserve(FLAGS_threads);
  for (uint64_t t = 0; t < FLAGS_threads; ++t) {
    threads.emplace_back(thread);
  }

  baton.post();

  for (auto& thread : threads) {
    thread.join();
  }

  printf(
      "open()\n  minimum: %" PRIu64 " ns\n  average: %" PRIu64 " ns\n",
      combined_open.getMinimum(),
      combined_open.getAverage(FLAGS_threads * FLAGS_iterations));
  printf(
      "close()\n  minimum: %" PRIu64 " ns\n  average: %" PRIu64 " ns\n",
      combined_close.getMinimum(),
      combined_close.getAverage(FLAGS_threads * FLAGS_iterations));
}
