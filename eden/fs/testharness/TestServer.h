/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include <folly/experimental/TestUtil.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class EdenServer;

/**
 * TestServer helps create an EdenServer object for use in unit tests.
 *
 * It contains a temporary directory object, and an EdenServer.
 */
class TestServer {
 public:
  TestServer();
  ~TestServer();

  AbsolutePath getTmpDir() const;

  EdenServer& getServer() {
    return *server_;
  }

 private:
  static std::unique_ptr<EdenServer> createServer(AbsolutePathPiece tmpDir);

  folly::test::TemporaryDirectory tmpDir_;
  std::unique_ptr<EdenServer> server_;
};

} // namespace eden
} // namespace facebook
