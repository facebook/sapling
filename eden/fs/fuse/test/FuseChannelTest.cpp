/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/FuseChannel.h"

#include <folly/experimental/logging/xlog.h>
#include <folly/io/async/ScopedEventBaseThread.h>
#include <gtest/gtest.h>
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/testharness/FakeFuse.h"

using namespace facebook::eden;
using namespace facebook::eden::fusell;
using namespace std::literals::chrono_literals;
using folly::ByteRange;
using folly::ScopedEventBaseThread;

namespace {
class TestDispatcher : public Dispatcher {
  using Dispatcher::Dispatcher;
};
} // namespace

TEST(FuseChannel, testDestroyNeverInitialized) {
  FakeFuse fake;
  ThreadLocalEdenStats stats;
  TestDispatcher dispatcher(&stats);
  AbsolutePath mountPath{"/fake/mount/path"};
  ScopedEventBaseThread eventBaseThread;
  size_t numThreads = 2;

  // Create a FuseChannel and then destroy it without ever calling initialize()
  {
    FuseChannel channel(
        fake.start(),
        mountPath,
        eventBaseThread.getEventBase(),
        numThreads,
        &dispatcher);
    (void)channel;
  }
}

// TODO: Add a test where we destroy the FuseChannel before initialization has
// completed.
//
// TODO: Add a test where an error occurs during initialization

TEST(FuseChannel, testInit) {
  FakeFuse fuse;
  ThreadLocalEdenStats stats;
  TestDispatcher dispatcher(&stats);
  AbsolutePath mountPath{"/fake/mount/path"};
  ScopedEventBaseThread eventBaseThread;
  size_t numThreads = 2;

  FuseChannel channel(
      fuse.start(),
      mountPath,
      eventBaseThread.getEventBase(),
      numThreads,
      &dispatcher);
  auto initFuture = channel.initialize(eventBaseThread.getEventBase());

  struct fuse_init_in initArg;
  initArg.major = FUSE_KERNEL_VERSION;
  initArg.minor = FUSE_KERNEL_MINOR_VERSION;
  initArg.max_readahead = 0;
  initArg.flags = 0;
  auto reqID = fuse.sendRequest(FUSE_INIT, 1, initArg);
  auto response = fuse.recvResponse();
  EXPECT_EQ(reqID, response.header.unique);
  EXPECT_EQ(0, response.header.error);
  EXPECT_EQ(
      sizeof(fuse_out_header) + sizeof(fuse_init_out), response.header.len);

  initFuture.get(100ms);

  // TODO: FuseChannel currently crashes on destruction unless the FUSE channel
  // has been closed and the session complete promise has been fulfilled.
  fuse.close();
  channel.getSessionCompleteFuture().get(100ms);
}
