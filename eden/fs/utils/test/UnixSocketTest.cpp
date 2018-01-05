/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/UnixSocket.h"
#include "eden/fs/utils/FutureUnixSocket.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/experimental/TestUtil.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using folly::ByteRange;
using folly::EventBase;
using folly::File;
using folly::IOBuf;
using folly::StringPiece;
using folly::checkUnixError;
using folly::errnoStr;
using folly::makeFuture;
using folly::test::TemporaryFile;
using std::make_unique;
using namespace std::chrono_literals;

using namespace facebook::eden;

namespace {
std::pair<folly::File, folly::File> createSocketPair() {
  std::array<int, 2> sockets;
  int rc = socketpair(AF_UNIX, SOCK_STREAM, 0, sockets.data());
  checkUnixError(rc, "socketpair failed");
  return std::make_pair(
      folly::File{sockets[0], true}, folly::File{sockets[1], true});
}

} // namespace

TEST(UnixSocket, getRemoteUID) {
  auto sockets = createSocketPair();
  EventBase evb;
  auto socket1 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.first));
  auto socket2 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.second));

  EXPECT_EQ(getuid(), socket2->getRemoteUID());
}

void testSendDataAndFiles(size_t dataSize, size_t numFiles) {
  XLOG(INFO) << "sending " << dataSize << " bytes, " << numFiles << " files";

  auto sockets = createSocketPair();
  EventBase evb;

  auto socket1 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.first));
  auto socket2 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.second));

  auto tmpFile = TemporaryFile("eden_test");
  struct stat tmpFileStat;
  if (fstat(tmpFile.fd(), &tmpFileStat) != 0) {
    ADD_FAILURE() << "fstat failed: " << errnoStr(errno);
  }

  auto sendBuf = IOBuf(IOBuf::CREATE, dataSize);
  memset(sendBuf.writableTail(), 'a', dataSize);
  sendBuf.append(dataSize);

  std::vector<File> files;
  for (size_t n = 0; n < numFiles; ++n) {
    files.emplace_back(tmpFile.fd(), /* ownsFd */ false);
  }
  socket1->send(UnixSocket::Message(sendBuf, std::move(files)))
      .then([] { XLOG(DBG3) << "send complete"; })
      .onError([](const folly::exception_wrapper& ew) {
        ADD_FAILURE() << "send error: " << ew.what();
      });

  folly::Optional<UnixSocket::Message> receivedMessage;
  socket2->receive(500ms)
      .then([&receivedMessage](UnixSocket::Message&& msg) {
        receivedMessage = std::move(msg);
      })
      .onError([](const folly::exception_wrapper& ew) {
        ADD_FAILURE() << "receive error: " << ew.what();
      })
      .ensure([&]() { evb.terminateLoopSoon(); });

  evb.loopForever();

  if (!receivedMessage) {
    ADD_FAILURE() << "no message received";
    return;
  }

  auto& msg = receivedMessage.value();

  EXPECT_EQ(dataSize, msg.data.computeChainDataLength());
  EXPECT_EQ(StringPiece{sendBuf.coalesce()}, StringPiece{msg.data.coalesce()});
  EXPECT_EQ(numFiles, msg.files.size());

  for (size_t n = 0; n < numFiles; ++n) {
    // The received file should be a different FD number than the one we
    // sent but should refer to the same underlying file.
    EXPECT_NE(tmpFile.fd(), msg.files[n].fd());
    struct stat receivedFileStat;
    if (fstat(msg.files[n].fd(), &receivedFileStat) != 0) {
      ADD_FAILURE() << "fstat failed: " << errnoStr(errno);
    }
    EXPECT_EQ(tmpFileStat.st_dev, receivedFileStat.st_dev);
    EXPECT_EQ(tmpFileStat.st_ino, receivedFileStat.st_ino);
  }
}

TEST(UnixSocket, sendDataAndFiles) {
  // Test various combinations of data length and number of files
  testSendDataAndFiles(5, 800);
  testSendDataAndFiles(0, 800);
  testSendDataAndFiles(5, 0);
  testSendDataAndFiles(0, 0);
  testSendDataAndFiles(4 * 1024 * 1024, 0);
  testSendDataAndFiles(4 * 1024 * 1024, 800);
}

TEST(FutureUnixSocket, receiveQueue) {
  auto sockets = createSocketPair();
  EventBase evb;

  auto socket1 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.first));
  auto socket2 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.second));

  std::vector<std::string> sendMessages = {
      "hello world",
      "test",
      "message 3",
      "",
      "stuff",
      "things",
      "foobar",
  };

  // Call receive multiple times on socket2
  std::vector<std::pair<int, UnixSocket::Message>> receivedMessages;
  for (size_t n = 0; n < sendMessages.size(); ++n) {
    auto future = socket2->receive(500ms)
                      .then([n, &receivedMessages](UnixSocket::Message&& msg) {
                        receivedMessages.emplace_back(n, std::move(msg));
                      })
                      .onError([n, &evb](const folly::exception_wrapper& ew) {
                        ADD_FAILURE()
                            << "receive " << n << " error: " << ew.what();
                        evb.terminateLoopSoon();
                      });
    // Terminate the event loop after the final receive
    if (n == sendMessages.size() - 1) {
      future.ensure([&evb]() { evb.terminateLoopSoon(); });
    }
  }

  // Now send the messages
  for (const auto& msg : sendMessages) {
    auto sendBuf = IOBuf(IOBuf::WRAP_BUFFER, ByteRange{StringPiece{msg}});
    socket1->send(std::move(sendBuf))
        .then([] { XLOG(DBG3) << "send complete"; })
        .onError([](const folly::exception_wrapper& ew) {
          ADD_FAILURE() << "send error: " << ew.what();
        });
  }

  evb.loopForever();

  for (size_t n = 0; n < sendMessages.size(); ++n) {
    if (n >= receivedMessages.size()) {
      ADD_FAILURE() << "missing message " << n << " from receive queue";
      continue;
    }
    EXPECT_EQ(n, receivedMessages[n].first);
    EXPECT_EQ(
        StringPiece{sendMessages[n]},
        StringPiece{receivedMessages[n].second.data.coalesce()});
  }
  EXPECT_EQ(sendMessages.size(), receivedMessages.size());
}
