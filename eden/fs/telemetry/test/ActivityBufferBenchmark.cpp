/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/File.h>
#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/stop_watch.h>
#include "eden/fs/utils/SpawnedProcess.h"

using namespace facebook::eden;

namespace {

constexpr uint32_t kNumFiles = 500;
constexpr uint32_t kNumWriteIterations = 10;
constexpr size_t kPageSize = 4096;

DEFINE_uint64(filesize, kPageSize, "File size in bytes");

SpawnedProcess::Options pipe_stdout_opts() {
  SpawnedProcess::Options opts;
  opts.pipeStdout();
  return opts;
}

std::string get_checkout_id() {
  SpawnedProcess whereamiProcess({"hg", "whereami"}, pipe_stdout_opts());
  std::string id = whereamiProcess.communicate().first;
  whereamiProcess.wait();
  return id;
}

std::string file_name(uint32_t id) {
  return fmt::format("{}{}{}", "activity_buffer_benchmark_file", id, ".txt");
}

struct TemporaryFile {
  explicit TemporaryFile(uint32_t id)
      : file{file_name(id), O_CREAT | O_EXCL | O_WRONLY | O_CLOEXEC} {
    if (FLAGS_filesize == 0 || (FLAGS_filesize % kPageSize)) {
      throw std::invalid_argument{"file size must be multiple of page size"};
    }
    folly::checkUnixError(ftruncate(file.fd(), FLAGS_filesize), "ftruncate");
  }

  folly::File file;
};

double get_time_elapsed(folly::stop_watch<> watch) {
  return std::chrono::duration_cast<std::chrono::duration<double>>(
             watch.elapsed())
      .count();
}

void ActivityBuffer_repeatedly_create_inodes() {
  printf("Creating files...\n");
  folly::stop_watch<> file_create_timer;
  for (uint32_t id = 0; id < kNumFiles; id++) {
    TemporaryFile{id};
  }
  printf(
      "Average elapsed time for creating a file: %.6f s\n",
      get_time_elapsed(file_create_timer) / kNumFiles);

  printf("Committing changes...\n");
  auto parent_id = get_checkout_id();
  SpawnedProcess({"hg", "add", "."}, pipe_stdout_opts()).wait();
  SpawnedProcess({"hg", "commit", "-m", "ActivityBufferBenchmark In Progress"})
      .wait();
  auto child_id = get_checkout_id();

  char s[] = "Test Message";
  double total_write_time = 0;
  printf("Unmaterializing and Writing to Files...\n");
  for (uint32_t iteration = 0; iteration < kNumWriteIterations; iteration++) {
    SpawnedProcess({"hg", "checkout", "--clean", parent_id}, pipe_stdout_opts())
        .wait();
    SpawnedProcess({"hg", "checkout", child_id}, pipe_stdout_opts()).wait();
    for (uint32_t id = 0; id < kNumFiles; id++) {
      folly::File file{file_name(id), O_WRONLY};
      folly::stop_watch<> file_write_timer;
      folly::checkUnixError(pwrite(file.fd(), s, sizeof(s), 0));
      total_write_time += get_time_elapsed(file_write_timer);
    }
  }
  printf(
      "Average elapsed time for writing to a file: %.6f s\n",
      total_write_time / (kNumFiles * kNumWriteIterations));

  printf("Uncommitting changes and deleting files...\n");
  SpawnedProcess({"hg", "uncommit"}).wait();
  for (uint32_t id = 0; id < kNumFiles; id++) {
    folly::checkUnixError(std::remove(file_name(id).c_str()));
  }
  SpawnedProcess({"hg", "addremove"}, pipe_stdout_opts()).wait();
  printf("ActivityBufferBenchmark finished\n");
}

} // namespace

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);
  ActivityBuffer_repeatedly_create_inodes();
  return 0;
}
