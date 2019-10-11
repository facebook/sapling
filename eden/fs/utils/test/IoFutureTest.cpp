/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/IoFuture.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/io/async/EventBase.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <sys/socket.h>
#include <chrono>

using facebook::eden::IoFuture;
using facebook::eden::waitForIO;
using folly::checkUnixError;
using folly::EventBase;
using folly::EventHandler;
using namespace std::chrono_literals;

namespace {
std::pair<folly::File, folly::File> createSocketPair() {
  std::array<int, 2> sockets;
  int rc = socketpair(AF_UNIX, SOCK_STREAM, 0, sockets.data());
  checkUnixError(rc, "socketpair failed");
  return std::make_pair(
      folly::File{sockets[0], true}, folly::File{sockets[1], true});
}
} // namespace

TEST(IoFuture, read) {
  auto sockets = createSocketPair();
  EventBase evb;

  // Wait for READ readiness
  auto f = waitForIO(&evb, sockets.first.fd(), EventHandler::READ, 1s);
  evb.loopOnce(EVLOOP_NONBLOCK);
  EXPECT_FALSE(f.isReady());

  auto bytesSent = send(sockets.second.fd(), "foo", 3, 0);
  checkUnixError(bytesSent, "send failed");
  EXPECT_FALSE(f.isReady());

  evb.loopOnce(EVLOOP_NONBLOCK);
  EXPECT_TRUE(f.isReady());
}

TEST(IoFuture, readTimeout) {
  auto sockets = createSocketPair();
  EventBase evb;

  // Wait for READ readiness
  auto f = waitForIO(&evb, sockets.first.fd(), EventHandler::READ, 10ms)
               .ensure([&evb] { evb.terminateLoopSoon(); });
  evb.loopForever();
  ASSERT_TRUE(f.isReady());
  EXPECT_THROW_ERRNO(std::move(f).get(), ETIMEDOUT);
}

TEST(IoFuture, multiRead) {
  auto sockets = createSocketPair();
  EventBase evb;

  // Re-use the same IoFuture object for multiple reads.
  IoFuture iof{&evb, sockets.first.fd()};

  // Wait for writability.  This should be immediately ready.
  auto writeF = iof.wait(EventHandler::WRITE, 1s);
  evb.loopOnce();
  EXPECT_TRUE(writeF.isReady());
  EXPECT_EQ(EventHandler::WRITE, std::move(writeF).get());

  // Wait for readability.
  auto readF1 = iof.wait(EventHandler::READ, 1s);
  EXPECT_FALSE(readF1.isReady());
  auto bytesSent = send(sockets.second.fd(), "foo", 3, 0);
  checkUnixError(bytesSent, "send failed");
  evb.loopOnce();
  EXPECT_TRUE(readF1.isReady());
  EXPECT_EQ(EventHandler::READ, readF1.wait().value());
  EXPECT_FALSE(readF1.hasException());

  // Read the data so the socket no longer has read data pending.
  std::array<char, 8> buf;
  auto bytesRead =
      recv(sockets.first.fd(), buf.data(), buf.size(), MSG_DONTWAIT);
  EXPECT_EQ(bytesRead, 3);

  // Wait for readability again, but expect it to time out this time.
  auto readF2 = iof.wait(EventHandler::READ, 20ms);
  EXPECT_FALSE(readF2.isReady());
  evb.loopOnce();
  ASSERT_TRUE(readF2.isReady());
  EXPECT_THROW_ERRNO(std::move(readF2).get(), ETIMEDOUT);

  // Try calling iof.wait() twice in a row, even though the
  // first one did not finish.  This should fail the earlier future with an
  // ECANCELED error.
  auto readF3 = iof.wait(EventHandler::READ, 1s);
  EXPECT_FALSE(readF3.isReady());
  auto readF4 = iof.wait(EventHandler::READ, 1s);
  ASSERT_TRUE(readF3.isReady());
  EXPECT_THROW_ERRNO(std::move(readF3).get(), ECANCELED);
  EXPECT_FALSE(readF4.isReady());

  bytesSent = send(sockets.second.fd(), "bar", 3, 0);
  checkUnixError(bytesSent, "send failed");
  evb.loopOnce();
  ASSERT_TRUE(readF4.isReady());
  EXPECT_EQ(EventHandler::READ, std::move(readF4).get());
  bytesRead = recv(sockets.first.fd(), buf.data(), buf.size(), MSG_DONTWAIT);
  EXPECT_EQ(bytesRead, 3);
}
