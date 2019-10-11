/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/fuse/FuseChannel.h"

#include <folly/Random.h>
#include <folly/logging/xlog.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <unordered_map>
#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/TestDispatcher.h"
#include "eden/fs/tracing/EdenStats.h"
#include "eden/fs/utils/ProcessNameCache.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::ByteRange;
using folly::Future;
using folly::Promise;
using folly::Random;
using folly::Unit;
using std::make_unique;
using std::unique_ptr;

namespace {

// Most of the tests wait on Futures to complete.
// Define a timeout just to make sure the tests don't hang if there's a problem
// and a future never completes.  1 second seems to be long enough for the tests
// to pass even when the system is under fairly heavy CPU load.
constexpr auto kTimeout = 1s;

fuse_entry_out genRandomLookupResponse(uint64_t nodeid) {
  fuse_entry_out response;
  response.nodeid = nodeid;
  response.generation = Random::rand64();
  response.entry_valid = Random::rand64();
  response.attr_valid = Random::rand64();
  response.entry_valid_nsec = Random::rand32();
  response.attr_valid_nsec = Random::rand32();
  response.attr.ino = nodeid;
  response.attr.size = Random::rand64();
  response.attr.blocks = Random::rand64();
  response.attr.atime = Random::rand64();
  response.attr.mtime = Random::rand64();
  response.attr.ctime = Random::rand64();
  response.attr.atimensec = Random::rand32();
  response.attr.mtimensec = Random::rand32();
  response.attr.ctimensec = Random::rand32();
  response.attr.mode = Random::rand32();
  response.attr.nlink = Random::rand32();
  response.attr.uid = Random::rand32();
  response.attr.gid = Random::rand32();
  response.attr.rdev = Random::rand32();
  response.attr.blksize = Random::rand32();
  response.attr.padding = Random::rand32();
  return response;
}

class FuseChannelTest : public ::testing::Test {
 protected:
  unique_ptr<FuseChannel, FuseChannelDeleter> createChannel(
      size_t numThreads = 2) {
    return unique_ptr<FuseChannel, FuseChannelDeleter>(new FuseChannel(
        fuse_.start(),
        mountPath_,
        numThreads,
        &dispatcher_,
        std::make_shared<ProcessNameCache>()));
  }

  FuseChannel::StopFuture performInit(
      FuseChannel* channel,
      uint32_t majorVersion = FUSE_KERNEL_VERSION,
      uint32_t minorVersion = FUSE_KERNEL_MINOR_VERSION,
      uint32_t maxReadahead = 0,
      uint32_t flags = 0) {
    auto initFuture = channel->initialize();
    EXPECT_FALSE(initFuture.isReady());

    // Send the INIT packet
    auto reqID =
        fuse_.sendInitRequest(majorVersion, minorVersion, maxReadahead, flags);

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
    return std::move(initFuture).get(kTimeout);
  }

  FakeFuse fuse_;
  EdenStats stats_;
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
  initFuture.wait(kTimeout);
}

TEST_F(FuseChannelTest, testInitUnmount) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Close the FakeFuse so that FuseChannel will think the mount point has been
  // unmounted.
  fuse_.close();

  // Wait for the FuseChannel to signal that it has finished.
  auto stopData = std::move(completeFuture).get(kTimeout);
  EXPECT_EQ(stopData.reason, FuseChannel::StopReason::UNMOUNTED);
  EXPECT_FALSE(stopData.fuseDevice);
}

TEST_F(FuseChannelTest, testTakeoverStop) {
  const uint32_t minorVersion = Random::rand32();
  const uint32_t maxReadahead = Random::rand32();
  constexpr uint32_t flags = FUSE_ASYNC_READ;
  auto channel = createChannel();
  auto completeFuture = performInit(
      channel.get(), FUSE_KERNEL_VERSION, minorVersion, maxReadahead, flags);

  channel->takeoverStop();

  // Wait for the FuseChannel to signal that it has finished.
  auto stopData = std::move(completeFuture).get(kTimeout);
  EXPECT_EQ(stopData.reason, FuseChannel::StopReason::TAKEOVER);
  // We should have received the FUSE device and valid settings information
  EXPECT_TRUE(stopData.fuseDevice);
  EXPECT_EQ(FUSE_KERNEL_VERSION, stopData.fuseSettings.major);
  EXPECT_EQ(minorVersion, stopData.fuseSettings.minor);
  EXPECT_EQ(maxReadahead, stopData.fuseSettings.max_readahead);
  EXPECT_EQ(flags, stopData.fuseSettings.flags);
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
  auto stopData = std::move(completeFuture).get(kTimeout);
  if (stopData.reason == FuseChannel::StopReason::UNMOUNTED) {
    EXPECT_FALSE(stopData.fuseDevice);
  } else if (stopData.reason == FuseChannel::StopReason::DESTRUCTOR) {
    EXPECT_TRUE(stopData.fuseDevice);
  } else {
    FAIL() << "unexpected FuseChannel stop reason: "
           << static_cast<int>(stopData.reason);
  }
}

TEST_F(FuseChannelTest, testInitErrorClose) {
  // Close the FUSE device while the FuseChannel is waiting on the INIT request
  auto channel = createChannel();
  auto initFuture = channel->initialize();
  fuse_.close();

  EXPECT_THROW_RE(
      std::move(initFuture).get(kTimeout),
      FuseDeviceUnmountedDuringInitialization,
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
      std::move(initFuture).get(kTimeout),
      std::runtime_error,
      "expected to receive FUSE_INIT for \"/fake/mount/path\" "
      "but got FUSE_LOOKUP");
}

TEST_F(FuseChannelTest, testInitErrorOldVersion) {
  auto channel = createChannel();
  auto initFuture = channel->initialize();

  // Send 2.7 as the FUSE version, which is too old
  struct fuse_init_in initArg = {};
  initArg.major = 2;
  initArg.minor = 7;
  initArg.max_readahead = 0;
  initArg.flags = 0;
  fuse_.sendRequest(FUSE_INIT, FUSE_ROOT_ID, initArg);

  EXPECT_THROW_RE(
      std::move(initFuture).get(kTimeout),
      std::runtime_error,
      "Unsupported FUSE kernel version 2.7 while initializing "
      "\"/fake/mount/path\"");
}

TEST_F(FuseChannelTest, testInitErrorShortPacket) {
  auto channel = createChannel();
  auto initFuture = channel->initialize();

  // Send a short message
  uint32_t body = 5;
  static_assert(
      sizeof(body) < sizeof(struct fuse_init_in),
      "we intend to send a body shorter than a fuse_init_in struct");
  fuse_.sendRequest(FUSE_INIT, FUSE_ROOT_ID, body);

  EXPECT_THROW_RE(
      std::move(initFuture).get(kTimeout),
      std::runtime_error,
      "received partial FUSE_INIT packet on mount \"/fake/mount/path\": "
      "size=44");
  static_assert(
      sizeof(fuse_in_header) + sizeof(uint32_t) == 44,
      "validate the size in our error message check");
}

TEST_F(FuseChannelTest, testDestroyWithPendingRequests) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Send several lookup requests
  auto id1 = fuse_.sendLookup(FUSE_ROOT_ID, "foobar");
  auto id2 = fuse_.sendLookup(FUSE_ROOT_ID, "some_file.txt");
  auto id3 = fuse_.sendLookup(FUSE_ROOT_ID, "main.c");

  auto req1 = dispatcher_.waitForLookup(id1);
  auto req2 = dispatcher_.waitForLookup(id2);
  auto req3 = dispatcher_.waitForLookup(id3);

  // Destroy the channel object
  channel.reset();

  // The completion future still should not be ready, since the lookup
  // requests are still pending.
  EXPECT_FALSE(completeFuture.isReady());

  auto checkLookupResponse = [](const FakeFuse::Response& response,
                                uint64_t requestID,
                                fuse_entry_out expected) {
    EXPECT_EQ(requestID, response.header.unique);
    EXPECT_EQ(0, response.header.error);
    EXPECT_EQ(
        sizeof(fuse_out_header) + sizeof(fuse_entry_out), response.header.len);
    EXPECT_EQ(
        ByteRange(
            reinterpret_cast<const uint8_t*>(&expected), sizeof(expected)),
        ByteRange(response.body.data(), response.body.size()));
  };

  // Respond to the lookup requests
  auto response1 = genRandomLookupResponse(9);
  req1.promise.setValue(response1);
  auto received = fuse_.recvResponse();
  checkLookupResponse(received, id1, response1);

  // We don't have to respond in order; respond to request 3 before 2
  auto response3 = genRandomLookupResponse(19);
  req3.promise.setValue(response3);
  received = fuse_.recvResponse();
  checkLookupResponse(received, id3, response3);

  // The completion future still shouldn't be ready since there is still 1
  // request outstanding.
  EXPECT_FALSE(completeFuture.isReady());

  auto response2 = genRandomLookupResponse(12);
  req2.promise.setValue(response2);
  received = fuse_.recvResponse();
  checkLookupResponse(received, id2, response2);

  // The completion future should be ready now that the last request
  // is done.
  EXPECT_TRUE(completeFuture.isReady());
  std::move(completeFuture).get(kTimeout);
}

// Test for getOutstandingRequest().
// It will generate few fuse request and verify the output of
// getOutstandingRequests() against them.
TEST_F(FuseChannelTest, getOutstandingRequests) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Send several lookup requests
  auto id1 = fuse_.sendLookup(FUSE_ROOT_ID, "foobar");
  auto id2 = fuse_.sendLookup(FUSE_ROOT_ID, "some_file.txt");
  auto id3 = fuse_.sendLookup(FUSE_ROOT_ID, "main.c");

  std::unordered_set<unsigned int> requestIds = {id1, id2, id3};

  auto req1 = dispatcher_.waitForLookup(id1);
  auto req2 = dispatcher_.waitForLookup(id2);
  auto req3 = dispatcher_.waitForLookup(id3);

  std::vector<fuse_in_header> outstandingCalls =
      channel->getOutstandingRequests();

  EXPECT_EQ(outstandingCalls.size(), 3);

  for (const auto& call : outstandingCalls) {
    EXPECT_EQ(FUSE_ROOT_ID, call.nodeid);
    EXPECT_EQ(FUSE_LOOKUP, call.opcode);
    EXPECT_EQ(1, requestIds.count(call.unique));
  }
}

TEST_F(FuseChannelTest, interruptLookups) {
  auto channel = createChannel();
  auto completeFuture = performInit(channel.get());

  // Send a bunch of lookup requests followed immediately by an interrupt
  // request that cancels the corresponding lookup request. We are trying
  // to exercise the codepaths here where handling of the interrupt request
  // may be running concurrently with the launching of the original request
  // on a different thread.

  for (int i = 0; i < 5000; ++i) {
    auto requestId = fuse_.sendLookup(FUSE_ROOT_ID, "foo");

    fuse_interrupt_in interruptData;
    interruptData.unique = requestId;

    (void)fuse_.sendRequest(FUSE_INTERRUPT, FUSE_ROOT_ID, interruptData);

    // For now FuseChannel never actually interrupts requests, so the
    // dispatcher will definitely receive the request.
    // We may need to change this code in the future if we do add true
    // interrupt support to FuseChannel.
    auto req = dispatcher_.waitForLookup(requestId);

    auto nodeId = 5 + i * 7;
    auto response = genRandomLookupResponse(nodeId);
    req.promise.setValue(response);

    auto received = fuse_.recvResponse();
    EXPECT_EQ(requestId, received.header.unique);
  }
}
