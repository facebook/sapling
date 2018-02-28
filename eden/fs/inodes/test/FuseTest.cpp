/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/utils/UnboundedQueueThreadPool.h"

#include <folly/experimental/logging/xlog.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/ScopedEventBaseThread.h>

using namespace facebook::eden;
using namespace std::literals::chrono_literals;
using folly::ScopedEventBaseThread;
using std::make_shared;

TEST(FuseTest, initMount) {
  ScopedEventBaseThread evbThread("test.main");

  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test/test.c", "testy tests");
  TestMount testMount{builder1};

  auto fuse = make_shared<FakeFuse>();
  testMount.registerFakeFuse(fuse);

  constexpr size_t threadCount = 2;
  auto threadPool =
      make_shared<UnboundedQueueThreadPool>(threadCount, "EdenCPUThread");
  auto initFuture =
      testMount.getEdenMount()
          ->startFuse(evbThread.getEventBase(), threadPool, folly::none)
          .then([] { XLOG(INFO) << "startFuse() succeeded"; })
          .onError([&](const folly::exception_wrapper& ew) {
            ADD_FAILURE() << "startFuse() failed: " << folly::exceptionStr(ew);
          });

  struct fuse_init_in initArg;
  initArg.major = FUSE_KERNEL_VERSION;
  initArg.minor = FUSE_KERNEL_MINOR_VERSION;
  initArg.max_readahead = 0;
  initArg.flags = 0;
  auto reqID = fuse->sendRequest(FUSE_INIT, 1, initArg);
  auto response = fuse->recvResponse();
  EXPECT_EQ(reqID, response.header.unique);
  EXPECT_EQ(0, response.header.error);
  EXPECT_EQ(
      sizeof(fuse_out_header) + sizeof(fuse_init_out), response.header.len);

  // TODO: EdenMount & FuseChannel currently have synchronization bugs where
  // they can cause invalid memory accesses if we do not wait for the init
  // future to complete before destroying them.
  initFuture.get(100ms);

  // TODO: FuseChannel currently crashes on destruction unless the FUSE channel
  // has been closed and the session complete promise has been fulfilled.
  fuse->close();
  testMount.getEdenMount()->getFuseCompletionFuture().get(100ms);
}
