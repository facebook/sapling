/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <boost/regex.hpp>
#include <folly/experimental/TestUtil.h>
#include <folly/io/async/ScopedEventBaseThread.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <proxygen/httpserver/HTTPServer.h>
#include <proxygen/httpserver/RequestHandler.h>
#include <proxygen/httpserver/ResponseBuilder.h>
#include <proxygen/httpserver/ScopedHTTPServer.h>
#include <proxygen/lib/http/HTTPCommonHeaders.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/mononoke/MononokeHttpBackingStore.h"
#include "eden/fs/utils/ServiceAddress.h"

using namespace facebook::eden;
using namespace proxygen;
using folly::ScopedEventBaseThread;
using folly::SocketAddress;

using BlobContents = std::map<std::string, std::string>;

namespace {

class Handler {
 public:
  explicit Handler(const BlobContents& blobs)
      : regex_(
            "^(/repo/blob/(.*)|"
            "/repo/tree/(.*)|"
            "/repo/changeset/(.*))$"),
        blobs_(blobs) {}

  void operator()(
      const proxygen::HTTPMessage& headers,
      std::unique_ptr<folly::IOBuf> /* requestBody */,
      ResponseBuilder& responseBuilder) {
    if (headers.getHeaders()
            .getSingleOrEmpty(proxygen::HTTP_HEADER_HOST)
            .empty()) {
      responseBuilder.status(400, "bad request: Host header is missing")
          .body("Host header is missing");
      return;
    }
    boost::cmatch m;
    auto match = boost::regex_match(headers.getPath().c_str(), m, regex_);
    if (match) {
      std::string content;
      if (blobs_.find(m[2]) != blobs_.end()) {
        content = blobs_[m[2]];
      } else if (blobs_.find(m[3]) != blobs_.end()) {
        content = blobs_[m[3]];
      } else if (blobs_.find(m[4]) != blobs_.end()) {
        content = blobs_[m[4]];
      } else {
        responseBuilder.status(404, "not found").body("cannot find content");
        return;
      }
      // Split the data in two to make sure that client's onBody() callback
      // works fine
      responseBuilder.status(200, "OK").send();

      for (auto c : content) {
        // Send characters one by one to make sure client's onBody() methods
        // works correctly.
        responseBuilder.body(c).send();
      }
    } else {
      responseBuilder.status(404, "not found").body("malformed url");
    }
  }

 private:
  boost::regex regex_;
  BlobContents blobs_;
};

} // namespace

class MononokeHttpBackingStoreTest : public ::testing::Test {
 protected:
  std::unique_ptr<ScopedHTTPServer> createServer() {
    return ScopedHTTPServer::start(Handler(getBlobs()));
  }

  BlobContents getBlobs() {
    BlobContents blobs = {
        std::make_pair(kZeroHash.toString(), "fileblob"),
        std::make_pair(emptyhash.toString(), ""),
        std::make_pair(malformedhash.toString(), "{"),
        std::make_pair(
            treehash.toString(),
            R"([{"hash": "b80de5d138758541c5f05265ad144ab9fa86d1db", "name": "a", "type": "file"},
                {"hash": "b8e02f6433738021a065f94175c7cd23db5f05be", "name": "b", "type": "file"},
                {"hash": "3333333333333333333333333333333333333333", "name": "dir", "type": "tree"},
                {"hash": "4444444444444444444444444444444444444444", "name": "exec", "type": "executable"},
                {"hash": "5555555555555555555555555555555555555555", "name": "link", "type": "symlink"}
            ])"),
        std::make_pair(
            commithash.toString(),
            R"({
              "manifest": "2222222222222222222222222222222222222222",
              "author": "John Doe <example@fb.com>",
              "comment": "a commit"
            })")};
    return blobs;
  }
  std::unique_ptr<ServiceAddress> getServerAddress(ScopedHTTPServer* server) {
    return std::make_unique<ServiceAddress>(
        "localhost", server->getAddresses()[0].address.getPort());
  }

  Hash emptyhash{"1111111111111111111111111111111111111111"};
  Hash treehash{"2222222222222222222222222222222222222222"};
  Hash commithash{"3333333333333333333333333333333333333333"};
  Hash malformedhash{"9999999999999999999999999999999999999999"};
  folly::EventBase mainEventBase;
};

TEST_F(MononokeHttpBackingStoreTest, testGetBlob) {
  auto server = createServer();
  auto blobs = getBlobs();

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(400),
      evbThread.getEventBase(),
      nullptr);
  auto blob = store.getBlob(kZeroHash).get();
  auto buf = blob->getContents();
  EXPECT_EQ(blobs[kZeroHash.toString()], buf.moveToFbString());
}

TEST_F(MononokeHttpBackingStoreTest, testConnectFailed) {
  // To get a port that is guaranteed to reject connections,
  // bind to an ephemeral port but never call listen()
  SocketAddress address;
  address.setFromLocalPort(uint16_t(0));
  auto serverSocket = folly::AsyncServerSocket::newSocket();
  serverSocket->bind(address);
  address = serverSocket->getAddress();
  auto service =
      std::make_unique<ServiceAddress>("localhost", address.getPort());

  auto blobs = getBlobs();

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      std::move(service),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  try {
    store.getBlob(kZeroHash).get();
    // Request should fail
    EXPECT_TRUE(false);
  } catch (const std::runtime_error&) {
  }
}

TEST_F(MononokeHttpBackingStoreTest, testEmptyBuffer) {
  auto server = createServer();
  auto blobs = getBlobs();

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  auto blob = store.getBlob(emptyhash).get();
  auto buf = blob->getContents();
  EXPECT_EQ(blobs[emptyhash.toString()], buf.moveToFbString());
}

TEST_F(MononokeHttpBackingStoreTest, testGetTree) {
  auto server = createServer();
  auto blobs = getBlobs();

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  auto tree = store.getTree(treehash).get();
  auto tree_entries = tree->getTreeEntries();

  std::vector<TreeEntry> expected_entries{
      TreeEntry(
          Hash("b80de5d138758541c5f05265ad144ab9fa86d1db"),
          "a",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("b8e02f6433738021a065f94175c7cd23db5f05be"),
          "b",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("3333333333333333333333333333333333333333"),
          "dir",
          TreeEntryType::TREE),
      TreeEntry(
          Hash("4444444444444444444444444444444444444444"),
          "exec",
          TreeEntryType::EXECUTABLE_FILE),
      TreeEntry(
          Hash("5555555555555555555555555555555555555555"),
          "link",
          TreeEntryType::SYMLINK),
  };

  Tree expected_tree(std::move(expected_entries), treehash);
  EXPECT_TRUE(expected_tree == *tree);
}

TEST_F(MononokeHttpBackingStoreTest, testMalformedGetTree) {
  auto server = createServer();
  auto blobs = getBlobs();

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  EXPECT_THROW(store.getTree(malformedhash).get(), std::exception);
}

TEST_F(MononokeHttpBackingStoreTest, testGetTreeForCommit) {
  auto server = createServer();
  auto blobs = getBlobs();
  auto commithash = this->commithash;

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  auto tree = store.getTreeForCommit(commithash).get();
  auto tree_entries = tree->getTreeEntries();

  std::vector<TreeEntry> expected_entries{
      TreeEntry(
          Hash("b80de5d138758541c5f05265ad144ab9fa86d1db"),
          "a",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("b8e02f6433738021a065f94175c7cd23db5f05be"),
          "b",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("3333333333333333333333333333333333333333"),
          "dir",
          TreeEntryType::TREE),
      TreeEntry(
          Hash("4444444444444444444444444444444444444444"),
          "exec",
          TreeEntryType::EXECUTABLE_FILE),
      TreeEntry(
          Hash("5555555555555555555555555555555555555555"),
          "link",
          TreeEntryType::SYMLINK),
  };

  Tree expected_tree(std::move(expected_entries), treehash);
  EXPECT_TRUE(expected_tree == *tree);
}

TEST_F(MononokeHttpBackingStoreTest, testGetTreeForManifest) {
  auto server = createServer();
  auto blobs = getBlobs();
  auto commithash = this->commithash;
  auto manifesthash = this->treehash;

  ScopedEventBaseThread evbThread;
  MononokeHttpBackingStore store(
      getServerAddress(server.get()),
      "repo",
      std::chrono::milliseconds(300),
      evbThread.getEventBase(),
      nullptr);
  auto tree = store.getTreeForManifest(commithash, manifesthash).get();
  auto tree_entries = tree->getTreeEntries();

  std::vector<TreeEntry> expected_entries{
      TreeEntry(
          Hash("b80de5d138758541c5f05265ad144ab9fa86d1db"),
          "a",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("b8e02f6433738021a065f94175c7cd23db5f05be"),
          "b",
          TreeEntryType::REGULAR_FILE),
      TreeEntry(
          Hash("3333333333333333333333333333333333333333"),
          "dir",
          TreeEntryType::TREE),
      TreeEntry(
          Hash("4444444444444444444444444444444444444444"),
          "exec",
          TreeEntryType::EXECUTABLE_FILE),
      TreeEntry(
          Hash("5555555555555555555555555555555555555555"),
          "link",
          TreeEntryType::SYMLINK),
  };

  Tree expected_tree(std::move(expected_entries), treehash);
  EXPECT_TRUE(expected_tree == *tree);
}
