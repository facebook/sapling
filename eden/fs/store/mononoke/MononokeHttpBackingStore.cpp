/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/mononoke/MononokeHttpBackingStore.h"

#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventBaseManager.h>
#include <folly/io/async/SSLOptions.h>
#include <folly/json.h>
#include <proxygen/lib/http/HTTPConnector.h>
#include <proxygen/lib/http/session/HTTPUpstreamSession.h>
#include <proxygen/lib/utils/URL.h>
#include <servicerouter/client/cpp2/ServiceRouter.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/mononoke/MononokeAPIUtils.h"
#include "eden/fs/utils/ServiceAddress.h"

using folly::IOBuf;
using folly::make_exception_wrapper;
using proxygen::ErrorCode;
using proxygen::HTTPException;
using proxygen::HTTPHeaders;
using proxygen::HTTPMessage;
using proxygen::HTTPTransaction;
using proxygen::UpgradeProtocol;
using proxygen::URL;

namespace facebook {
namespace eden {
namespace {

using IOBufPromise = folly::Promise<std::unique_ptr<folly::IOBuf>>;

// Callback that processes response for getblob request.
// Note: because this callback deletes itself, it must be allocated on the heap!
class MononokeCallback : public proxygen::HTTPConnector::Callback,
                         public proxygen::HTTPTransaction::Handler {
 public:
  MononokeCallback(const proxygen::URL& url, IOBufPromise&& promise)
      : promise_(std::move(promise)), url_(url) {}

  void connectSuccess(proxygen::HTTPUpstreamSession* session) override {
    auto txn = session->newTransaction(this);
    HTTPMessage message;
    message.setMethod(proxygen::HTTPMethod::GET);
    message.setURL(url_.makeRelativeURL());
    message.getHeaders().add("Host", url_.getHost());
    txn->sendHeaders(message);
    txn->sendEOM();
    session->closeWhenIdle();
  }

  void connectError(const folly::AsyncSocketException& ex) override {
    // handler won't be used anymore, should be safe to delete
    promise_.setException(make_exception_wrapper<std::runtime_error>(
        folly::to<std::string>("mononoke connection error: ", ex.what())));
    delete this;
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
            "mononoke request ",
            url_.getUrl(),
            " failed: ",
            status_code_->getStatusCode(),
            " ",
            status_code_->getStatusMessage(),
            ". body size: ",
            body_ ? body_->computeChainDataLength() : 0);
        promise_.setException(
            make_exception_wrapper<std::runtime_error>(error_msg));
      }
    }

    /*
    From proxygen source code comments (HTTPTransaction.h):
        The Handler deletes itself at some point after the Transaction
        has detached from it.

    After detachTransaction() call Handler won't be used and should be safe to
    delete.
    */
    delete this;
  }

  void onHeadersComplete(std::unique_ptr<HTTPMessage> msg) noexcept override {
    status_code_ = std::move(msg);
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
    auto exception = make_exception_wrapper<std::runtime_error>(
        folly::to<std::string>("mononoke HTTP error: ", error.describe()));
    error_.swap(exception);
  }

  void onEgressPaused() noexcept override {}

  void onEgressResumed() noexcept override {}

  void onPushedTransaction(HTTPTransaction* /* txn */) noexcept override {}

  void onGoaway(ErrorCode /* code */) noexcept override {}

 private:
  bool isSuccessfulStatusCode() {
    // 2xx are successful status codes
    return (status_code_->getStatusCode() / 100) == 2;
  }

  IOBufPromise promise_;
  proxygen::URL url_;
  Hash hash_;
  std::string repo_;
  std::unique_ptr<HTTPMessage> status_code_;
  std::unique_ptr<folly::IOBuf> body_{nullptr};
  // Pointer to the last IOBuf in a chain
  folly::IOBuf* last_{nullptr};
  folly::exception_wrapper error_{nullptr};
};
} // namespace

MononokeHttpBackingStore::MononokeHttpBackingStore(
    std::unique_ptr<ServiceAddress> service,
    const std::string& repo,
    const std::chrono::milliseconds& timeout,
    folly::Executor* executor,
    const std::shared_ptr<folly::SSLContext> sslContext)
    : service_(std::move(service)),
      repo_(repo),
      timeout_(timeout),
      executor_(executor),
      sslContext_(sslContext) {}

MononokeHttpBackingStore::~MononokeHttpBackingStore() {}

folly::Future<std::unique_ptr<Tree>> MononokeHttpBackingStore::getTree(
    const Hash& id) {
  return folly::via(executor_)
      .thenValue([this, id](auto&&) { return sendRequest("tree", id); })
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return parseMononokeTree(std::move(buf), id);
      });
}

folly::SemiFuture<std::unique_ptr<Blob>> MononokeHttpBackingStore::getBlob(
    const Hash& id) {
  return folly::via(executor_)
      .thenValue([this, id](auto&&) { return sendRequest("blob", id); })
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return std::make_unique<Blob>(id, *buf);
      });
}

folly::Future<std::unique_ptr<Tree>> MononokeHttpBackingStore::getTreeForCommit(
    const Hash& commitID) {
  return folly::via(executor_)
      .thenValue([this, commitID](auto&&) {
        return sendRequest("changeset", commitID);
      })
      .thenValue([&](std::unique_ptr<folly::IOBuf>&& buf) {
        auto s = buf->moveToFbString();
        auto parsed = folly::parseJson(s);
        auto hash = Hash(parsed.at("manifest").asString());
        return getTree(hash);
      });
}

folly::Future<std::unique_ptr<Tree>>
MononokeHttpBackingStore::getTreeForManifest(
    const Hash& /* commitID */,
    const Hash& manifestID) {
  return getTree(manifestID);
}

folly::Future<SocketAddressWithHostname>
MononokeHttpBackingStore::getAddress() {
  return folly::via(executor_, [this] {
    auto addr = service_->getSocketAddressBlocking();
    if (!addr) {
      throw std::runtime_error("could not get address of the server");
    }
    return std::move(*addr);
  });
}

folly::Future<std::unique_ptr<IOBuf>> MononokeHttpBackingStore::sendRequest(
    folly::StringPiece endpoint,
    const Hash& id) {
  return getAddress().thenValue([=](SocketAddressWithHostname addr) {
    return sendRequestImpl(addr, endpoint, id);
  });
}

folly::Future<std::unique_ptr<IOBuf>> MononokeHttpBackingStore::sendRequestImpl(
    SocketAddressWithHostname addr,
    folly::StringPiece endpoint,
    const Hash& id) {
  const auto& [socketAddress, host] = addr;
  URL url(folly::sformat(
      "https://{}:{}/{}/{}/{}",
      host,
      socketAddress.getPort(),
      repo_,
      endpoint,
      id.toString()));

  auto eventBase = folly::EventBaseManager::get()->getEventBase();
  IOBufPromise promise;
  auto future = promise.getFuture();
  // MononokeCallback deletes itself - see detachTransaction() method
  MononokeCallback* callback = new MononokeCallback(url, std::move(promise));
  // It is moved into the .then() lambda below and destroyed there
  folly::HHWheelTimer::UniquePtr timer{folly::HHWheelTimer::newTimer(
      eventBase,
      std::chrono::milliseconds(folly::HHWheelTimer::DEFAULT_TICK_INTERVAL),
      folly::AsyncTimeout::InternalEnum::NORMAL,
      timeout_)};

  // It is moved into the .then() lambda below and deleted there
  auto connector =
      std::make_unique<proxygen::HTTPConnector>(callback, timer.get());

  static const folly::AsyncSocket::OptionMap opts{
      {{SOL_SOCKET, SO_REUSEADDR}, 1}};

  if (sslContext_ != nullptr) {
    connector->connectSSL(
        eventBase,
        socketAddress,
        sslContext_,
        nullptr,
        timeout_,
        opts,
        folly::AsyncSocket::anyAddress(),
        host);
  } else {
    connector->connect(eventBase, socketAddress, timeout_, opts);
  }

  /* capture `connector` to make sure it stays alive for the duration of the
     connection */
  return std::move(future).thenValue(
      [connector = std::move(connector), timer = std::move(timer)](
          std::unique_ptr<folly::IOBuf>&& buf) { return std::move(buf); });
}

} // namespace eden
} // namespace facebook
