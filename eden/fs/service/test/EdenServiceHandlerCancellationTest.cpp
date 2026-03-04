/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServiceHandler.h"

#include <chrono>
#include <thread>
#include <vector>

#include <folly/CancellationToken.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <gtest/gtest.h>

namespace facebook::eden {

class EdenServiceHandlerCancellationTest : public ::testing::Test {};

TEST_F(EdenServiceHandlerCancellationTest, RequestCancellationStates) {
  RequestCancellationInfo defaultInfo;
  EXPECT_EQ(RequestStatus::ACTIVE, defaultInfo.status);
  EXPECT_FALSE(defaultInfo.isCancelable());
  EXPECT_FALSE(defaultInfo.requestCancellation());

  auto uncancelableInfo = RequestCancellationInfo::createUncancelable();
  EXPECT_EQ(RequestStatus::UNCANCELABLE, uncancelableInfo.status);
  EXPECT_FALSE(uncancelableInfo.isCancelable());
  EXPECT_FALSE(uncancelableInfo.requestCancellation());

  folly::CancellationSource source;
  auto token = source.getToken();
  RequestCancellationInfo cancelableInfo(std::move(source), "testEndpoint");

  EXPECT_EQ(RequestStatus::ACTIVE, cancelableInfo.status);
  EXPECT_TRUE(cancelableInfo.isCancelable());
  EXPECT_FALSE(token.isCancellationRequested());

  EXPECT_TRUE(cancelableInfo.requestCancellation());
  EXPECT_EQ(RequestStatus::REQUESTED, cancelableInfo.status);
  EXPECT_TRUE(token.isCancellationRequested());

  EXPECT_FALSE(cancelableInfo.requestCancellation());
  EXPECT_EQ(RequestStatus::REQUESTED, cancelableInfo.status);
}

TEST_F(EdenServiceHandlerCancellationTest, NoCancellation) {
  folly::CancellationSource cancelSource;
  auto cancellationToken = cancelSource.getToken();

  RequestCancellationInfo requestInfo(std::move(cancelSource), "testEndpoint");
  EXPECT_EQ(RequestStatus::ACTIVE, requestInfo.status);
  EXPECT_TRUE(requestInfo.isCancelable());

  auto performLongRunningOperation =
      [&](const folly::CancellationToken& token) -> bool {
    // Simulate work with enough iterations to make it meaningful
    // but still very fast for CI. No sleep calls needed.
    for (int i = 0; i < 1000; ++i) {
      if (token.isCancellationRequested()) {
        return false;
      }
      // Yield to allow other threads to run, but don't sleep
      std::this_thread::yield();
    }
    return true;
  };

  auto operationResult = performLongRunningOperation(cancellationToken);
  EXPECT_TRUE(operationResult);

  EXPECT_EQ(RequestStatus::ACTIVE, requestInfo.status);
}

TEST_F(EdenServiceHandlerCancellationTest, CancellationDuringOperation) {
  folly::CancellationSource cancelSource;
  auto cancellationToken = cancelSource.getToken();

  RequestCancellationInfo requestInfo(std::move(cancelSource), "testEndpoint");

  std::atomic<bool> operationCancelled{false};

  folly::Promise<folly::Unit> startPromise;
  auto startFuture = startPromise.getFuture();

  std::thread operationThread([&]() {
    startPromise.setValue(folly::Unit{});
    // Simulate a long-running operation that checks for cancellation
    while (!cancellationToken.isCancellationRequested()) {
      std::this_thread::yield();
    }
    operationCancelled = true;
  });

  // Wait for operation to start before requesting cancellation
  std::move(startFuture).wait();

  EXPECT_TRUE(requestInfo.requestCancellation());

  operationThread.join();

  EXPECT_TRUE(operationCancelled.load());

  EXPECT_EQ(RequestStatus::REQUESTED, requestInfo.status);
}

TEST_F(EdenServiceHandlerCancellationTest, ConcurrentTokenUsage) {
  const int numThreads = 4;

  folly::CancellationSource source;
  auto token = source.getToken();
  RequestCancellationInfo requestInfo(std::move(source), "testEndpoint");

  std::vector<std::thread> threads;
  threads.reserve(numThreads);
  std::atomic<int> readyThreads{0};
  std::atomic<int> checksBeforeCancellation{0};
  std::atomic<int> checksAfterCancellation{0};
  std::atomic<int> checksCompleted{0};

  folly::Promise<folly::Unit> allReadyPromise;
  auto allReadyFuture = allReadyPromise.getFuture();

  folly::Promise<folly::Unit> allCheckedPromise;
  auto allCheckedFuture = allCheckedPromise.getFuture();

  std::atomic<bool> cancellationSignaled{false};

  for (int t = 0; t < numThreads; ++t) {
    threads.emplace_back([&]() {
      // Signal ready and wait for all threads
      if (readyThreads.fetch_add(1) == numThreads - 1) {
        allReadyPromise.setValue(folly::Unit{});
      }

      while (readyThreads.load() < numThreads) {
        std::this_thread::yield();
      }

      if (!token.isCancellationRequested()) {
        checksBeforeCancellation.fetch_add(1);
      }

      // Signal that this thread completed its pre-cancellation check
      if (checksCompleted.fetch_add(1) == numThreads - 1) {
        allCheckedPromise.setValue(folly::Unit{});
      }

      // Wait for cancellation signal
      while (!cancellationSignaled.load()) {
        std::this_thread::yield();
      }

      if (token.isCancellationRequested()) {
        checksAfterCancellation.fetch_add(1);
      }
    });
  }

  // Wait for all threads to complete their pre-cancellation checks
  std::move(allReadyFuture).wait();
  std::move(allCheckedFuture).wait();

  EXPECT_TRUE(requestInfo.requestCancellation());

  // Signal all threads that cancellation has been requested
  cancellationSignaled = true;

  for (auto& thread : threads) {
    thread.join();
  }

  EXPECT_EQ(numThreads, checksBeforeCancellation.load());
  EXPECT_EQ(numThreads, checksAfterCancellation.load());

  EXPECT_EQ(RequestStatus::REQUESTED, requestInfo.status);
}

} // namespace facebook::eden
