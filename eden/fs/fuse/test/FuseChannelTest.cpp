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
using folly::Future;
using folly::ScopedEventBaseThread;
using folly::Unit;
using std::make_unique;

namespace {
class TestDispatcher : public Dispatcher {
  using Dispatcher::Dispatcher;
};

class FuseChannelTest : public ::testing::Test {
 protected:
  void SetUp() override {
    eventBaseThread_ = make_unique<ScopedEventBaseThread>();
  }

  std::unique_ptr<FuseChannel> createChannel(size_t numThreads = 2) {
    return make_unique<FuseChannel>(
        fuse_.start(),
        mountPath_,
        eventBaseThread_->getEventBase(),
        numThreads,
        &dispatcher_);
  }

  void performInit(FuseChannel* channel) {
    auto initFuture = channel->initialize();
    EXPECT_FALSE(initFuture.isReady());

    // Send the INIT packet
    auto reqID = fuse_.sendInitRequest();

    // Wait for the INIT response
    auto response = fuse_.recvResponse();
    EXPECT_EQ(reqID, response.header.unique);
    EXPECT_EQ(0, response.header.error);
    EXPECT_EQ(
        sizeof(fuse_out_header) + sizeof(fuse_init_out), response.header.len);
    EXPECT_EQ(sizeof(fuse_init_out), response.body.size());

    // The init future should be ready very shortly after we receive the INIT
    // response.  The FuseChannel initialization thread makes the future ready
    // shortly after sending the INIT response.
    initFuture.get(100ms);
  }

  FakeFuse fuse_;
  ThreadLocalEdenStats stats_;
  TestDispatcher dispatcher_{&stats_};
  AbsolutePath mountPath_{"/fake/mount/path"};
  std::unique_ptr<ScopedEventBaseThread> eventBaseThread_;
};

} // namespace

TEST_F(FuseChannelTest, testDestroyNeverInitialized) {
  // Create a FuseChannel and then destroy it without ever calling initialize()
  auto channel = createChannel();
}

TEST_F(FuseChannelTest, testInitDestroy) {
  // Initialize the FuseChannel then immediately invoke its destructor
  // without explicitly requesting it to stop or receiving a close on the fUSE
  // device.
  auto channel = createChannel();
  performInit(channel.get());
}

// TODO: Add a test where we destroy the FuseChannel before initialization has
// completed.
//
// TODO: Add a test where an error occurs during initialization

TEST_F(FuseChannelTest, testInitUnmount) {
  auto channel = createChannel();
  performInit(channel.get());

  // Close the FakeFuse so that FuseChannel will think the mount point has been
  // unmounted.
  fuse_.close();

  // Wait for the FuseChannel to signal that it has finished.
  auto stopReason = channel->getSessionCompleteFuture().get(100ms);
  EXPECT_EQ(stopReason, FuseChannel::StopReason::UNMOUNTED);
}

TEST_F(FuseChannelTest, testInitUnmountRace) {
  auto channel = createChannel();
  performInit(channel.get());
  auto completeFuture = channel->getSessionCompleteFuture();

  // Close the FakeFuse so that FuseChannel will think the mount point has been
  // unmounted.  We then immediately destroy the FuseChannel without waiting
  // for the session complete future, so that destruction and unmounting race.
  fuse_.close();
  channel.reset();

  // Wait for the session complete future now.
  auto stopReason = completeFuture.get(100ms);
  EXPECT_TRUE(
      stopReason == FuseChannel::StopReason::UNMOUNTED ||
      stopReason == FuseChannel::StopReason::DESTRUCTOR)
      << "unexpected FuseChannel stop reason: " << static_cast<int>(stopReason);
}
