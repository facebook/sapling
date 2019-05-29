/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
