/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <iostream>
#include <numeric>
#include <stdexcept>
#include <thread>
#include <vector>

#ifndef _WIN32
#include <sys/xattr.h>
#endif

#include <benchmark/benchmark.h>
#include <boost/filesystem.hpp>

#include <folly/File.h>
#include <folly/container/Array.h>
#include <folly/init/Init.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/logging/xlog.h>
#include <folly/synchronization/test/Barrier.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>

#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

using namespace facebook::eden;
using namespace boost::filesystem;

DEFINE_int64(threads, 1, "The number of concurrent Thrift client threads");
DEFINE_string(repo, "", "Path to Eden repository");
DEFINE_string(
    interface,
    "",
    "Way to get sha1s. Options are: "
    "\"thrift\" (query EdenFS's thrift interface), "
    "\"filesystem\" (getxattr calls through the filesystem), or "
    "\"both\" (try both and display separate results).");

bool shouldRecordThriftSamples(std::string& interface) {
  return interface == "both" || interface == "thrift";
}

bool shouldRecordFilesystemSamples(std::string& interface) {
  return interface == "both" || interface == "filesystem";
}

/**
 * Record a sample in `samples` of how long it takes to read a file's sha1 from
 * EdenFS's thrift interface.
 */
void recordThriftSample(
    std::string& file,
    boost::filesystem::path& repo_path,
    std::unique_ptr<EdenServiceAsyncClient>& client,
    uint64_t& sample) {
  auto start = getTime();
  std::vector<SHA1Result> res;
  // see notes in recordFilesystemSample about these DoNotOptimize protecting
  // ordering here
  benchmark::DoNotOptimize(file);
  auto sync = SyncBehavior{};
  client->sync_getSHA1(res, repo_path.native(), {file}, sync);
  benchmark::DoNotOptimize(res);
  auto duration = std::chrono::nanoseconds(getTime() - start);
  sample =
      std::chrono::duration_cast<std::chrono::microseconds>(duration).count();

  if (UNLIKELY(res.empty())) {
    throw std::runtime_error("No results!");
  }
  if (UNLIKELY(
          res.size() != 1 || res.at(0).getType() == SHA1Result::Type::error)) {
    throw res.at(0).get_error();
  }
}

/**
 * Record a sample in `samples` of how long it takes to read a file's sha1 using
 * a call through the filesystem (getxattr).
 */
void recordFilesystemSample(std::string& file, uint64_t& sample) {
  constexpr size_t res_size = 40;
  std::array<char, res_size> res{};
  auto start = getTime();
  // The DoNotOptimize calls "fence" in the sha1 access and make it reliable to
  // time:
  // https://stackoverflow.com/questions/37786547/enforcing-statement-order-in-c
  benchmark::DoNotOptimize(file); // this is just used so that the ordering of
                                  // start time and the sha1 access can not be
                                  // reordered by the compiler
  ssize_t success{0};
#ifdef __APPLE__
  success = getxattr(file.c_str(), "user.sha1", res.data(), res_size, 0, 0);
#elif __linux__
  success = getxattr(file.c_str(), "user.sha1", res.data(), res_size);
#endif
  benchmark::DoNotOptimize(res); // this is just used so that the ordering of
                                 // recording the end time and the sha1 access
                                 // can not be reordered by the compiler
  auto duration = std::chrono::nanoseconds(getTime() - start);
  sample =
      std::chrono::duration_cast<std::chrono::microseconds>(duration).count();

  if (UNLIKELY(success == -1)) {
    throw std::system_error{std::error_code{errno, std::system_category()}};
  }
}

/**
 * Calculate some standard statistics for the given samples and display them.
 */
void calculateStats(
    std::vector<uint64_t>& samples,
    unsigned nthreads,
    unsigned samples_per_thread) {
  if (UNLIKELY(samples.empty())) {
    throw std::runtime_error("No samples to calculate stats for!");
  }
  std::sort(samples.begin(), samples.end());
  double avg =
      std::accumulate(samples.begin(), samples.end(), 0.0) / samples.size();
  fmt::print("avg: {:6f} us\n", avg);
  fmt::print("min: {} us\n", samples.at(0));
  const unsigned nsamples = samples_per_thread * nthreads;
  std::array<unsigned, 3> pcts = folly::make_array<unsigned>(5, 50, 95);
  for (const auto& p : pcts) {
    fmt::print(
        "p{}: {} us\n",
        static_cast<uint64_t>(p),
        samples.at(p * nsamples / 100));
  }
}

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  if (!FLAGS_threads) {
    std::cerr << "Must specify nonzero number of threads" << std::endl;
    gflags::ShowUsageWithFlags(argv[0]);
    return 1;
  }
  if (FLAGS_repo.empty()) {
    std::cerr << "Must specify a repository root" << std::endl;
    gflags::ShowUsageWithFlagsRestrict(argv[0], __FILE__);
    return 1;
  }

  auto real_path = realpath(FLAGS_repo.c_str(), nullptr);
  if (!real_path) {
    perror("realpath on given repo failed");
    return 1;
  }
  SCOPE_EXIT {
    free(real_path);
  };
  path repo_path = real_path;

  if ((argc - 1) < FLAGS_threads) {
    std::cerr
        << "Must specify a set of files to query, at least one per thread."
        << " files to query: " << argc - 1
        << " threads to run: " << FLAGS_threads << std::endl;
    return 1;
  }

  if ((argc - 1) % FLAGS_threads != 0) {
    std::cerr << "Each thread needs the same number of files to sample."
              << " files to query: " << argc - 1
              << " threads to run: " << FLAGS_threads << std::endl;
    return 1;
  }

#ifdef WIN32
  if (shouldRecordFilesystemSamples(FLAGS_interface)) {
    std::cerr << "Filesystem sha1 not currently supported" << std::endl;
    return 1;
  }
#endif

  std::vector<std::string> thrift_files;
  std::vector<std::string> filesystem_files;
  for (int i = 1; i < argc; ++i) {
    if (shouldRecordThriftSamples(FLAGS_interface)) {
      thrift_files.emplace_back(argv[i]);
    }
    if (shouldRecordFilesystemSamples(FLAGS_interface)) {
      filesystem_files.emplace_back((repo_path / argv[i]).native());
    }
  }

  const auto socket_path = repo_path / ".eden" / "socket";
  const unsigned nthreads = FLAGS_threads;
  // This number should stay a power of two, to avoid cache line ping-ponging
  // on the boundaries.
  const unsigned samples_per_thread = 8192;

  std::vector<std::thread> threads;
  folly::test::Barrier gate{static_cast<unsigned>(nthreads)};
  std::vector<uint64_t> thrift_samples(nthreads * samples_per_thread);
  std::vector<uint64_t> filesystem_samples(nthreads * samples_per_thread);
  // we are just going to do calculations based on the thrift_files size, so
  // let's assert the assumption that they have the same size
  XCHECK_EQ(
      thrift_files.size(),
      filesystem_files.size(),
      "thrift and filesystem number of files must be the same");
  for (unsigned thread_number = 0; thread_number < nthreads; ++thread_number) {
    threads.emplace_back([thread_number,
                          &gate,
                          &socket_path,
                          &repo_path,
                          &thrift_samples,
                          &filesystem_samples,
                          &thrift_files,
                          &filesystem_files,
                          &interface = FLAGS_interface] {
      // The order of these variables matters, the client MUST be
      // destroyed before the event base because the client
      // destructor is gonna touch the eventbase.
      folly::EventBase eventBase;
      std::unique_ptr<EdenServiceAsyncClient> client;
      if (shouldRecordThriftSamples(interface)) {
        auto socket = folly::AsyncSocket::newSocket(
            &eventBase,
            folly::SocketAddress::makeFromPath(
                socket_path.string<std::string>()));
        auto channel =
            apache::thrift::HeaderClientChannel::newChannel(std::move(socket));
        client = std::make_unique<EdenServiceAsyncClient>(std::move(channel));
      }

      gate.wait();
      for (unsigned j = 0; j < samples_per_thread; ++j) {
        auto files_index = j * thread_number % thrift_files.size();
        auto samples_index = thread_number * samples_per_thread + j;
        if (shouldRecordThriftSamples(interface)) {
          recordThriftSample(
              thrift_files[files_index],
              repo_path,
              client,
              thrift_samples[samples_index]);
        }

        if (shouldRecordFilesystemSamples(interface)) {
          recordFilesystemSample(
              filesystem_files[files_index], filesystem_samples[samples_index]);
        }
      }
    });
  }

  for (auto& thread : threads) {
    thread.join();
  }

  if (shouldRecordThriftSamples(FLAGS_interface)) {
    std::cout << "Thrift Statistics: " << std::endl;
    calculateStats(thrift_samples, nthreads, samples_per_thread);
    std::cout << std::endl;
  }

  if (shouldRecordFilesystemSamples(FLAGS_interface)) {
    std::cout << "Filesystem Statistics: " << std::endl;
    calculateStats(filesystem_samples, nthreads, samples_per_thread);
    std::cout << std::endl;
  }

  return 0;
}
