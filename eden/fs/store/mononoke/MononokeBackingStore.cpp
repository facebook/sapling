/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MononokeBackingStore.h"

#include <eden/fs/model/Blob.h>
#include <eden/fs/model/Hash.h>
#include <eden/fs/model/Tree.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/io/async/EventBase.h>
#include <proxygen/lib/http/HTTPConnector.h>
#include <proxygen/lib/http/session/HTTPUpstreamSession.h>
#include <proxygen/lib/utils/URL.h>

using folly::makeFuture;
using folly::make_exception_wrapper;
using proxygen::ErrorCode;
using proxygen::HTTPException;
using proxygen::HTTPHeaders;
using proxygen::HTTPMessage;
using proxygen::HTTPTransaction;
using proxygen::UpgradeProtocol;

namespace facebook {
namespace eden {
namespace {

using IOBufPromise = folly::Promise<std::unique_ptr<folly::IOBuf>>;

// Callback that processes response for getblob request.
// Url format: /REPONAME/blob/HASH/
class GetBlobCallback : public proxygen::HTTPConnector::Callback,
                        public proxygen::HTTPTransaction::Handler {
 public:
  GetBlobCallback(
      const Hash& hash,
      IOBufPromise&& promise,
      const std::string& repo)
      : promise_(std::move(promise)), hash_(hash), repo_(repo) {}

  virtual void connectSuccess(proxygen::HTTPUpstreamSession* session) override {
    auto txn = session->newTransaction(this);
    HTTPMessage message;
    message.setMethod(proxygen::HTTPMethod::GET);
    proxygen::URL url(folly::sformat("/{}/blob/{}/", repo_, hash_.toString()));
    message.setURL(url.makeRelativeURL());
    txn->sendHeaders(message);
    txn->sendEOM();
    session->closeWhenIdle();
  }

  void connectError(const folly::AsyncSocketException& /* ex */) override {
    promise_.setException(
        make_exception_wrapper<std::runtime_error>("connect error"));
  }

  // We don't send anything back, so ignore this callback
  void setTransaction(HTTPTransaction* /* txn */) noexcept override {}

  void detachTransaction() noexcept override {
    if (error_) {
      promise_.setException(error_);
    } else {
      if (body_ == nullptr) {
        // Make sure we return empty buffer and not nullptr to the caller.
        // It can happen if blob is empty.
        body_ = folly::IOBuf::create(0);
      }
      if (isSuccessfulStatusCode()) {
        promise_.setValue(std::move(body_));
      } else {
        auto error_msg = folly::to<std::string>(
            "request failed: ",
            status_code_,
            ", ",
            body_ ? body_->moveToFbString() : "");
        promise_.setException(
            make_exception_wrapper<std::runtime_error>(error_msg));
      }
    }
  }

  void onHeadersComplete(std::unique_ptr<HTTPMessage> msg) noexcept override {
    status_code_ = msg->getStatusCode();
  }

  void onBody(std::unique_ptr<folly::IOBuf> chain) noexcept override {
    if (!body_) {
      body_.swap(chain);
      last_ = body_->next();
    } else {
      last_->appendChain(std::move(chain));
      last_ = last_->next();
    }
  }

  void onChunkHeader(size_t /* length */) noexcept override {}

  void onChunkComplete() noexcept override {}

  void onTrailers(
      std::unique_ptr<HTTPHeaders> /* trailers */) noexcept override {}

  void onEOM() noexcept override {}

  void onUpgrade(UpgradeProtocol /* protocol */) noexcept override {}

  void onError(const HTTPException& error) noexcept override {
    auto exception =
        make_exception_wrapper<std::runtime_error>(error.describe());
    error_.swap(exception);
  }

  void onEgressPaused() noexcept override {}

  void onEgressResumed() noexcept override {}

  void onPushedTransaction(HTTPTransaction* /* txn */) noexcept override {}

  void onGoaway(ErrorCode /* code */) noexcept override {}

 private:
  bool isSuccessfulStatusCode() {
    // 2xx are successful status codes
    return (status_code_ / 100) == 2;
  }

  IOBufPromise promise_;
  Hash hash_;
  std::string repo_;
  uint16_t status_code_{0};
  std::unique_ptr<folly::IOBuf> body_{nullptr};
  // Pointer to the last IOBuf in a chain
  folly::IOBuf* last_{nullptr};
  folly::exception_wrapper error_{nullptr};
};
} // namespace

MononokeBackingStore::MononokeBackingStore(
    const folly::SocketAddress& sa,
    const std::string& repo,
    const std::chrono::milliseconds& timeout)
    : sa_(sa), repo_(repo), timeout_(timeout) {}

MononokeBackingStore::~MononokeBackingStore() {}

folly::Future<std::unique_ptr<Tree>> MononokeBackingStore::getTree(
    const Hash& /* id */) {
  throw std::runtime_error("not implemented");
}

// TODO(stash): make the call async
folly::Future<std::unique_ptr<Blob>> MononokeBackingStore::getBlob(
    const Hash& id) {
  folly::EventBase evb;

  IOBufPromise promise;
  auto future = promise.getFuture();
  GetBlobCallback callback(id, std::move(promise), repo_);

  folly::HHWheelTimer::UniquePtr timer{folly::HHWheelTimer::newTimer(
      &evb,
      std::chrono::milliseconds(folly::HHWheelTimer::DEFAULT_TICK_INTERVAL),
      folly::AsyncTimeout::InternalEnum::NORMAL,
      timeout_)};

  proxygen::HTTPConnector connector(&callback, timer.get());

  const folly::AsyncSocket::OptionMap opts{{{SOL_SOCKET, SO_REUSEADDR}, 1}};
  connector.connect(&evb, this->sa_, timeout_, opts);
  evb.loop();

  return future.then([id](std::unique_ptr<folly::IOBuf>&& buf) {
    return makeFuture(std::make_unique<Blob>(id, *buf));
  });
}

folly::Future<std::unique_ptr<Tree>> MononokeBackingStore::getTreeForCommit(
    const Hash& commitID) {
  throw std::runtime_error("not implemented");
}
} // namespace eden
} // namespace facebook
