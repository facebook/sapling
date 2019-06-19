/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/UnixSocket.h"
#include "eden/fs/utils/FutureUnixSocket.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/Random.h>
#include <folly/Range.h>
#include <folly/String.h>
#include <folly/experimental/TestUtil.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <optional>
#include <random>

#include "eden/fs/testharness/TempFile.h"

using folly::ByteRange;
using folly::checkUnixError;
using folly::errnoStr;
using folly::EventBase;
using folly::File;
using folly::IOBuf;
using folly::makeFuture;
using folly::StringPiece;
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

  EXPECT_EQ(getuid(), socket1->getRemoteUID());
  EXPECT_EQ(getuid(), socket2->getRemoteUID());
}

struct DataSize {
  explicit DataSize(size_t total, size_t maxChunk = 0)
      : totalSize{total}, maxChunkSize{maxChunk} {}

  size_t totalSize{0};
  size_t maxChunkSize{0};
};

void testSendDataAndFiles(DataSize dataSize, size_t numFiles) {
  XLOG(INFO) << "sending " << dataSize.totalSize << " bytes, " << numFiles
             << " files, with max chunk size of " << dataSize.maxChunkSize;

  auto sockets = createSocketPair();
  EventBase evb;

  auto socket1 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.first));
  auto socket2 = make_unique<FutureUnixSocket>(&evb, std::move(sockets.second));

  // Set a fairly large send and receive timeout for this test.
  // On Mac OS X the send can take a fairly long-ish time when sending
  // more than 1MB or so.
  constexpr auto timeout = 10s;
  socket1->setSendTimeout(timeout);

  auto tmpFile = makeTempFile();
  struct stat tmpFileStat;
  if (fstat(tmpFile.fd(), &tmpFileStat) != 0) {
    ADD_FAILURE() << "fstat failed: " << errnoStr(errno);
  }

  std::unique_ptr<IOBuf> sendBuf;
  if (dataSize.maxChunkSize == 0) {
    // Send everything in one chunk
    sendBuf = IOBuf::create(dataSize.totalSize);
    memset(sendBuf->writableTail(), 'a', dataSize.totalSize);
    sendBuf->append(dataSize.totalSize);
  } else {
    // Use a fixed seed so we get repeatable results across unit test runs.
    std::mt19937 rng;
    rng.seed(1);

    // Break the data into randomly sized chunks, from 0 to maxChunkSize bytes
    uint8_t byteValue = 1;
    size_t bytesLeft = dataSize.totalSize;
    while (bytesLeft > 0) {
      auto chunkSize = folly::Random::rand32(dataSize.maxChunkSize, rng);
      if (chunkSize > bytesLeft) {
        chunkSize = bytesLeft;
      }
      // Request a minimum of 32 bytes just to ensure we allocate some data
      // rather than a null buffer if chunkSize is 0.  This shouldn't really
      // matter in practice, though.
      auto buf = IOBuf::create(std::max(chunkSize, 32U));
      memset(buf->writableTail(), byteValue, chunkSize);
      buf->append(chunkSize);
      bytesLeft -= chunkSize;
      ++byteValue; // Fill each chunk with a different byte value
      if (sendBuf == nullptr) {
        sendBuf = std::move(buf);
      } else {
        // Yes, unfortunately "prependChain()" is the method that appends this
        // buffer to the end of the IOBuf chain.  (The chain is a circularly
        // linked list, prepending immediately in front of the head effectively
        // appends to the end.)  I'm unfortunately to blame for this horrible
        // naming choice.
        sendBuf->prependChain(std::move(buf));
      }
    }
  }

  std::vector<File> files;
  for (size_t n = 0; n < numFiles; ++n) {
    files.emplace_back(tmpFile.fd(), /* ownsFd */ false);
  }
  socket1->send(UnixSocket::Message(sendBuf->cloneAsValue(), std::move(files)))
      .thenValue([](auto&&) { XLOG(DBG3) << "send complete"; })
      .thenError([](const folly::exception_wrapper& ew) {
        ADD_FAILURE() << "send error: " << ew.what();
      });

  std::optional<UnixSocket::Message> receivedMessage;
  socket2->receive(timeout)
      .thenValue([&receivedMessage](UnixSocket::Message&& msg) {
        receivedMessage = std::move(msg);
      })
      .thenError([](const folly::exception_wrapper& ew) {
        ADD_FAILURE() << "receive error: " << ew.what();
      })
      .ensure([&]() { evb.terminateLoopSoon(); });

  evb.loopForever();

  if (!receivedMessage) {
    ADD_FAILURE() << "no message received";
    return;
  }

  auto& msg = receivedMessage.value();

  EXPECT_EQ(dataSize.totalSize, msg.data.computeChainDataLength());
  EXPECT_EQ(StringPiece{sendBuf->coalesce()}, StringPiece{msg.data.coalesce()});
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
  testSendDataAndFiles(DataSize(5), 800);
  testSendDataAndFiles(DataSize(0), 800);
  testSendDataAndFiles(DataSize(5), 0);
  testSendDataAndFiles(DataSize(0), 0);
  testSendDataAndFiles(DataSize(4 * 1024 * 1024), 0);
  testSendDataAndFiles(DataSize(4 * 1024 * 1024), 800);
  testSendDataAndFiles(DataSize(32 * 1024 * 1024), 0);
  testSendDataAndFiles(DataSize(32 * 1024 * 1024), 800);

  // Send several MB of data split up into chunks of at most 1000 bytes.
  // This will result in a lot of iovecs to send.
  testSendDataAndFiles(DataSize(4 * 1024 * 1024, 1000), 800);
  testSendDataAndFiles(DataSize(32 * 1024 * 1024, 1000), 0);
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
    auto future =
        socket2->receive(500ms)
            .thenValue([n, &receivedMessages](UnixSocket::Message&& msg) {
              receivedMessages.emplace_back(n, std::move(msg));
            })
            .thenError([n, &evb](const folly::exception_wrapper& ew) {
              ADD_FAILURE() << "receive " << n << " error: " << ew.what();
              evb.terminateLoopSoon();
            });
    // Terminate the event loop after the final receive
    if (n == sendMessages.size() - 1) {
      std::move(future).ensure([&evb]() { evb.terminateLoopSoon(); });
    }
  }

  // Now send the messages
  for (const auto& msg : sendMessages) {
    auto sendBuf = IOBuf(IOBuf::WRAP_BUFFER, ByteRange{StringPiece{msg}});
    socket1->send(std::move(sendBuf))
        .thenValue([](auto&&) { XLOG(DBG3) << "send complete"; })
        .thenError([](const folly::exception_wrapper& ew) {
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

TEST(FutureUnixSocket, attachEventBase) {
  // A helper function to attach sockets to an EventBase, send a message, then
  // detach from the EventBase
  auto test = [&](EventBase* evb, FutureUnixSocket& s1, FutureUnixSocket& s2) {
    s1.attachEventBase(evb);
    s2.attachEventBase(evb);
    SCOPE_EXIT {
      s1.detachEventBase();
      s2.detachEventBase();
    };

    const std::string msgData(100, 'a');
    s1.send(UnixSocket::Message(IOBuf(IOBuf::COPY_BUFFER, msgData)))
        .thenValue([](auto&&) { XLOG(DBG3) << "send complete"; })
        .thenError([](const folly::exception_wrapper& ew) {
          ADD_FAILURE() << "send error: " << ew.what();
        });
    std::optional<UnixSocket::Message> receivedMessage;
    s2.receive(500ms)
        .thenValue([&receivedMessage](UnixSocket::Message&& msg) {
          receivedMessage = std::move(msg);
        })
        .thenError([](const folly::exception_wrapper& ew) {
          ADD_FAILURE() << "receive error: " << ew.what();
        })
        .ensure([&]() { evb->terminateLoopSoon(); });

    evb->loopForever();

    EXPECT_TRUE(receivedMessage.has_value());
    EXPECT_EQ(msgData, receivedMessage->data.moveToFbString().toStdString());
  };

  // Create two sockets that are initially not attached to an EventBase
  auto sockets = createSocketPair();
  auto socket1 = FutureUnixSocket(nullptr, std::move(sockets.first));
  auto socket2 = FutureUnixSocket(nullptr, std::move(sockets.second));

  // Test on one EventBase
  {
    EventBase evb1;
    test(&evb1, socket1, socket2);
  }
  // Now test using another EventBase
  {
    EventBase evb2;
    test(&evb2, socket2, socket1);
  }
}
