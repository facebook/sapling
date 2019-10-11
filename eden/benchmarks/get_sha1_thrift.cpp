/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <boost/filesystem.hpp>
#include <folly/Benchmark.h>
#include <folly/File.h>
#include <folly/container/Array.h>
#include <folly/init/Init.h>
#include <folly/synchronization/test/Barrier.h>
#include <thrift/lib/cpp/async/TAsyncSocket.h>
#include <thrift/lib/cpp2/async/HeaderClientChannel.h>
#include <iostream>
#include <numeric>
#include <thread>
#include <vector>

#include "eden/fs/benchharness/Bench.h"
#include "eden/fs/service/gen-cpp2/EdenService.h"

using namespace facebook::eden;
using namespace boost::filesystem;

DEFINE_uint64(threads, 1, "The number of concurrent Thrift client threads");
DEFINE_string(repo, "", "Path to Eden repository");

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

  std::vector<std::string> files;
  for (int i = 1; i < argc; ++i) {
    files.emplace_back(argv[i]);
  }

  if (files.size() < FLAGS_threads) {
    std::cerr << "Must specify a set of files to query, at least one per thread"
              << std::endl;
    return 1;
  }

  path repo_path = real_path;
  const auto socket_path = repo_path / ".eden" / "socket";
  const unsigned nthreads = FLAGS_threads;
  const auto samples_per_thread = 131072;

  std::vector<std::thread> threads;
  folly::test::Barrier gate{static_cast<unsigned>(nthreads)};
  std::vector<uint64_t> samples(nthreads * samples_per_thread);
  for (unsigned i = 0; i < nthreads; ++i) {
    threads.emplace_back(
        [i, &gate, &socket_path, &repo_path, &samples, &files] {
          // Setup a socket per-thread talking to eden
          auto sock_fd = socket(AF_LOCAL, SOCK_STREAM, 0);
          if (sock_fd == -1) {
            perror("Failed to create socket");
            return;
          }
          struct sockaddr_un addr;
          addr.sun_family = AF_UNIX;
          strncpy(addr.sun_path, socket_path.c_str(), 108);
          addr.sun_path[107] = '\0';
          auto rc =
              connect(sock_fd, (const struct sockaddr*)&addr, sizeof(addr));
          if (rc == -1) {
            perror("Failed to connect to socket");
            return;
          }
          folly::EventBase eventBase;
          auto socket = apache::thrift::async::TAsyncSocket::newSocket(
              &eventBase, sock_fd);
          auto channel = folly::to_shared_ptr(
              apache::thrift::HeaderClientChannel::newChannel(socket));
          auto client = std::make_unique<EdenServiceAsyncClient>(channel);

          gate.wait();
          for (auto j = 0; j < samples_per_thread; ++j) {
            std::vector<SHA1Result> res;
            auto start = getTime();
            folly::doNotOptimizeAway(files[i]);
            client->sync_getSHA1(res, repo_path.native(), {files[i]});
            folly::doNotOptimizeAway(res);
            auto duration = std::chrono::nanoseconds(getTime() - start);
            samples[i * samples_per_thread + j] =
                std::chrono::duration_cast<std::chrono::microseconds>(duration)
                    .count();
          }
        });
  }

  for (auto& thread : threads) {
    thread.join();
  }

  // calculate statistics
  std::sort(samples.begin(), samples.end());
  double avg =
      std::accumulate(samples.begin(), samples.end(), 0.0) / samples.size();
  std::cout << "avg: " << avg << "us" << std::endl;
  std::cout << "min: " << samples[0] << "us" << std::endl;
  const auto nsamples = samples_per_thread * nthreads;
  auto pct = folly::make_array(0.05, 0.5, 0.95);
  for (const auto& p : pct) {
    std::cout << "p" << static_cast<uint64_t>(p * 100) << ": "
              << samples[p * nsamples] << "us" << std::endl;
  }

  return 0;
}
