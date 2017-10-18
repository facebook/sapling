/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <boost/regex.hpp>
#include <folly/experimental/TestUtil.h>
#include <folly/experimental/logging/Init.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <proxygen/httpserver/HTTPServer.h>
#include <proxygen/httpserver/RequestHandler.h>
#include <proxygen/httpserver/ResponseBuilder.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/mononoke/MononokeBackingStore.h"

using namespace facebook::eden;
using namespace proxygen;
using folly::SocketAddress;

using BlobContents = std::map<std::string, std::string>;

class Handler : public proxygen::RequestHandler {
 public:
  explicit Handler(const BlobContents& blobs)
      : blob_regex_("^/repo/blob/(.*)/$"), path_(), blobs_(blobs) {}

  ~Handler() {}

  void onRequest(
      std::unique_ptr<proxygen::HTTPMessage> headers) noexcept override {
    path_ = headers->getPath();
  }

  void onBody(std::unique_ptr<folly::IOBuf> /* body */) noexcept override {}

  void onEOM() noexcept override {
    boost::cmatch m;
    auto match = boost::regex_match(path_.c_str(), m, blob_regex_);
    if (match && blobs_.find(m[1]) != blobs_.end()) {
      auto data = blobs_[m[1]];
      // Split the data in two to make sure that client's onBody() callback
      // works fine
      ResponseBuilder(downstream_).status(200, "OK").send();

      for (auto c : data) {
        // Send characters one by one to make sure client's onBody() methods
        // works correctly.
        ResponseBuilder(downstream_).body(c).send();
      }
      ResponseBuilder(downstream_).sendWithEOM();
    } else {
      ResponseBuilder(downstream_).status(404, "not found").sendWithEOM();
    }
  }

  void onUpgrade(proxygen::UpgradeProtocol /* proto */) noexcept override {}

  void requestComplete() noexcept override {
    delete this;
  }

  void onError(proxygen::ProxygenError /* err */) noexcept override {}

 private:
  boost::regex blob_regex_;
  std::string path_;
  BlobContents blobs_;
};

class HandlerFactory : public RequestHandlerFactory {
 public:
  explicit HandlerFactory(const BlobContents& blobs) : blobs_(blobs) {}

  void onServerStart(folly::EventBase* /*evb*/) noexcept override {}

  void onServerStop() noexcept override {}

  RequestHandler* onRequest(RequestHandler*, HTTPMessage*) noexcept override {
    return new Handler(blobs_);
  }

 private:
  BlobContents blobs_;
};

class MononokeBackingStoreTest : public ::testing::Test {
 protected:
  std::unique_ptr<HTTPServer> createServer() {
    std::string ip("localhost");
    auto port = 0; // choose any free port
    std::vector<HTTPServer::IPConfig> IPs = {
        {SocketAddress(ip, port, true), HTTPServer::Protocol::HTTP},
    };

    auto blobs = getBlobs();
    HTTPServerOptions options;
    options.threads = 1;
    options.handlerFactories =
        RequestHandlerChain().addThen<HandlerFactory>(blobs).build();
    auto server = folly::make_unique<HTTPServer>(std::move(options));
    server->bind(IPs);
    return server;
  }

  BlobContents getBlobs() {
    Hash emptyhash("1111111111111111111111111111111111111111");
    BlobContents blobs = {
        std::pair<std::string, std::string>(kZeroHash.toString(), "fileblob"),
        std::pair<std::string, std::string>(emptyhash.toString(), ""),
    };
    return blobs;
  }
};

TEST_F(MononokeBackingStoreTest, testGetBlob) {
  auto server = createServer();
  auto blobs = getBlobs();
  std::thread t([&]() {
    server->start([&server, &blobs]() {
      MononokeBackingStore store(
          server->addresses()[0].address,
          "repo",
          std::chrono::milliseconds(300));
      auto blob = store.getBlob(kZeroHash).get();
      auto buf = blob->getContents();
      EXPECT_EQ(blobs[kZeroHash.toString()], buf.moveToFbString());
      server->stop();
    });
  });

  t.join();
}

TEST_F(MononokeBackingStoreTest, testConnectFailed) {
  auto server = createServer();
  auto blobs = getBlobs();

  auto port = server->addresses()[0].address.getPort();
  auto sa = SocketAddress("localhost", port, true);
  MononokeBackingStore store(sa, "repo", std::chrono::milliseconds(300));
  try {
    store.getBlob(kZeroHash).get();
    // Request should fail
    EXPECT_TRUE(false);
  } catch (const std::runtime_error&) {
  }
}

TEST_F(MononokeBackingStoreTest, testEmptyBuffer) {
  auto server = createServer();
  auto blobs = getBlobs();
  std::thread t([&]() {
    server->start([&server, &blobs]() {
      Hash emptyhash("1111111111111111111111111111111111111111");
      MononokeBackingStore store(
          server->addresses()[0].address,
          "repo",
          std::chrono::milliseconds(300));
      auto blob = store.getBlob(emptyhash).get();
      auto buf = blob->getContents();
      EXPECT_EQ(blobs[emptyhash.toString()], buf.moveToFbString());
      server->stop();
    });
  });

  t.join();
}
