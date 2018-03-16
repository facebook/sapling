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
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/testharness/FakeFuse.h"

using namespace facebook::eden;
using namespace facebook::eden::fusell;
using namespace std::literals::chrono_literals;
using folly::ByteRange;
using folly::Future;
using folly::Unit;
using std::make_unique;

namespace {
class TestDispatcher : public Dispatcher {
  using Dispatcher::Dispatcher;
};

class FuseChannelTest : public ::testing::Test {
 protected:
  std::unique_ptr<FuseChannel> createChannel(size_t numThreads = 2) {
    return make_unique<FuseChannel>(
        fuse_.start(),
        mountPath_,
        numThreads,
        &dispatcher_);
  }

  FuseChannel::StopFuture performInit(FuseChannel* channel) {
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
    return initFuture.get(100ms);
  }

  FakeFuse fuse_;
  ThreadLocalEdenStats stats_;
  TestDispatcher dispatcher_{&stats_};
  AbsolutePath mountPath_{"/fake/mount/path"};
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

TEST_F(FuseChannelTest, testDestroyWithPendingInit) {
  // Create a FuseChannel, call initialize(), and then destroy the FuseChannel
  // without ever having seen the INIT request from the kernel.
  auto channel = createChannel();
  auto initFuture = channel->initialize();
  EXPECT_FALSE(initFuture.isReady());
}

TEST_F(FuseChannelTest, testInitDestroyRace) {
  // Send an INIT request and immediately destroy the FuseChannelTest
  // without waiting for initialization to complete.
  auto channel = createChannel();
  auto initFuture = channel->initialize();
  fuse_.sendInitRequest();
  channel.reset();

  // Wait for the initialization future to complete.
  // It's fine if it fails if the channel was destroyed before initialization
  // completed, or its fine if it succeeded first too.
  initFuture.wait(100ms);
}

TEST_F(FuseChannelTest, testInitUnmount) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Close the FakeFuse so that FuseChannel will think the mount point has been
  // unmounted.
  fuse_.close();

  // Wait for the FuseChannel to signal that it has finished.
  auto stopReason = std::move(completeFuture).get(100ms);
  EXPECT_EQ(stopReason, FuseChannel::StopReason::UNMOUNTED);
}

TEST_F(FuseChannelTest, testInitUnmountRace) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Close the FakeFuse so that FuseChannel will think the mount point has been
  // unmounted.  We then immediately destroy the FuseChannel without waiting
  // for the session complete future, so that destruction and unmounting race.
  fuse_.close();
  channel.reset();

  // Wait for the session complete future now.
  auto stopReason = std::move(completeFuture).get(100ms);
  EXPECT_TRUE(
      stopReason == FuseChannel::StopReason::UNMOUNTED ||
      stopReason == FuseChannel::StopReason::DESTRUCTOR)
      << "unexpected FuseChannel stop reason: " << static_cast<int>(stopReason);
}

TEST_F(FuseChannelTest, testInitErrorClose) {
  // Close the FUSE device while the FuseChannel is waiting on the INIT request
  auto channel = createChannel();
  auto initFuture = channel->initialize();
  fuse_.close();

  EXPECT_THROW_RE(
      initFuture.get(100ms),
      std::runtime_error,
      "FUSE mount \"/fake/mount/path\" was unmounted before we "
      "received the INIT packet");
}

TEST_F(FuseChannelTest, testInitErrorWrongPacket) {
  // Send a packet other than FUSE_INIT while the FuseChannel is waiting on the
  // INIT request
  auto channel = createChannel();
  auto initFuture = channel->initialize();

  // Use a fuse_init_in body, but FUSE_LOOKUP as the opcode
  struct fuse_init_in initArg = {};
  fuse_.sendRequest(FUSE_LOOKUP, FUSE_ROOT_ID, initArg);

  EXPECT_THROW_RE(
      initFuture.get(100ms),
      std::runtime_error,
      "expected to receive FUSE_INIT for \"/fake/mount/path\" "
      "but got FUSE_LOOKUP");
}

TEST_F(FuseChannelTest, testInitErrorOldVersion) {
  auto channel = createChannel();
  auto initFuture = channel->initialize();

  // Use a fuse_init_in body, but FUSE_LOOKUP as the opcode
  struct fuse_init_in initArg = {};
  initArg.major = 2;
  initArg.minor = 7;
  initArg.max_readahead = 0;
  initArg.flags = 0;
  fuse_.sendRequest(FUSE_INIT, FUSE_ROOT_ID, initArg);

  EXPECT_THROW_RE(
      initFuture.get(100ms),
      std::runtime_error,
      "Unsupported FUSE kernel version 2.7 while initializing "
      "\"/fake/mount/path\"");
}

TEST_F(FuseChannelTest, testInitErrorShortPacket) {
  auto channel = createChannel();
  auto initFuture = channel->initialize();

  // Use a fuse_init_in body, but FUSE_LOOKUP as the opcode
  uint32_t body = 5;
  fuse_.sendRequest(FUSE_INIT, FUSE_ROOT_ID, body);

  EXPECT_THROW_RE(
      initFuture.get(100ms),
      std::runtime_error,
      "received partial FUSE_INIT packet on mount \"/fake/mount/path\": "
      "size=44");
  static_assert(
      sizeof(fuse_in_header) + sizeof(uint32_t) == 44,
      "validate the size in our error message check");
}
