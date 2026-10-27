/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/futures/Future.h>
#include <folly/testing/TestUtil.h>
#include <chrono>
#include <optional>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/service/StartupStatusSubscriber.h"

namespace facebook::eden {

class BackingStoreFactory;
class EdenServer;

/**
 * TestServer helps create an EdenServer object for use in unit tests.
 *
 * It contains a temporary directory object, and an EdenServer.
 */
class TestServer {
 public:
  struct Options {
    bool enableNfsServer{false};
    // Override core:takeover-request-grace,
    // experimental:takeover-waits-for-requests and
    // core:takeover-recent-activity-window when set.
    std::optional<std::chrono::nanoseconds> takeoverRequestGrace;
    std::optional<bool> takeoverWaitsForRequests;
    std::optional<std::chrono::nanoseconds> takeoverRecentActivityWindow;
    // Serves the server's mounts. The default refuses to create any backing
    // store, so a test that mounts a checkout has to provide one.
    BackingStoreFactory* backingStoreFactory{nullptr};
  };

  TestServer();
  explicit TestServer(const Options& options);
  ~TestServer();

  AbsolutePath getTmpDir() const;
  void waitUntilReady();

  EdenServer& getServer() {
    return *server_;
  }

 private:
  static std::unique_ptr<EdenServer> createServer(
      AbsolutePathPiece tmpDir,
      std::shared_ptr<StartupStatusChannel> startStatusChannel,
      const Options& options);

  folly::test::TemporaryDirectory tmpDir_;
  folly::Future<folly::Unit> prepareResult_;
  std::unique_ptr<EdenServer> server_;
};

} // namespace facebook::eden
