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
#include <folly/io/async/EventBaseManager.h>
#include <folly/io/async/SSLOptions.h>
#include <folly/json.h>
#include <proxygen/lib/http/HTTPConnector.h>
#include <proxygen/lib/http/session/HTTPUpstreamSession.h>
#include <proxygen/lib/utils/URL.h>
#include <servicerouter/client/cpp2/ServiceRouter.h>

using folly::Future;
using folly::IOBuf;
using folly::make_exception_wrapper;
using folly::makeFuture;
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

  virtual void connectSuccess(proxygen::HTTPUpstreamSession* session) override {
    auto txn = session->newTransaction(this);
    HTTPMessage message;
    message.setMethod(proxygen::HTTPMethod::GET);
    message.setURL(url_.makeRelativeURL());
    txn->sendHeaders(message);
    txn->sendEOM();
    session->closeWhenIdle();
  }

  void connectError(const folly::AsyncSocketException& /* ex */) override {
    // handler won't be used anymore, should be safe to delete
    promise_.setException(
        make_exception_wrapper<std::runtime_error>("connect error"));
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
            "request failed: ",
            status_code_,
            ", ",
            body_ ? body_->moveToFbString() : "");
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
  proxygen::URL url_;
  Hash hash_;
  std::string repo_;
  uint16_t status_code_{0};
  std::unique_ptr<folly::IOBuf> body_{nullptr};
  // Pointer to the last IOBuf in a chain
  folly::IOBuf* last_{nullptr};
  folly::exception_wrapper error_{nullptr};
};

std::unique_ptr<Tree> convertBufToTree(
    std::unique_ptr<folly::IOBuf>&& buf,
    const Hash& id) {
  auto s = buf->moveToFbString();
  auto parsed = folly::parseJson(s);
  if (!parsed.isArray()) {
    throw std::runtime_error("malformed json: should be array");
  }

  std::vector<TreeEntry> entries;
  for (auto i = parsed.begin(); i != parsed.end(); ++i) {
    auto name = i->at("name").asString();
    auto hash = Hash(i->at("hash").asString());
    auto str_type = i->at("type").asString();
    TreeEntryType file_type;
    if (str_type == "file") {
      file_type = TreeEntryType::REGULAR_FILE;
    } else if (str_type == "tree") {
      file_type = TreeEntryType::TREE;
    } else if (str_type == "executable") {
      file_type = TreeEntryType::EXECUTABLE_FILE;
    } else if (str_type == "symlink") {
      file_type = TreeEntryType::SYMLINK;
    } else {
      throw std::runtime_error("unknown file type");
    }
    entries.push_back(TreeEntry(hash, name, file_type));
  }
  return std::make_unique<Tree>(std::move(entries), id);
}

} // namespace

// This constructor should only be used in testing.
MononokeBackingStore::MononokeBackingStore(
    const folly::SocketAddress& socketAddress,
    const std::string& repo,
    const std::chrono::milliseconds& timeout,
    folly::Executor* executor,
    const std::shared_ptr<folly::SSLContext> sslContext)
    : socketAddress_(folly::Optional<folly::SocketAddress>(socketAddress)),
      repo_(repo),
      timeout_(timeout),
      executor_(executor),
      sslContext_(sslContext) {}

MononokeBackingStore::MononokeBackingStore(
    const std::string& repo,
    const std::chrono::milliseconds& timeout,
    folly::Executor* executor,
    const std::shared_ptr<folly::SSLContext> sslContext)
    : socketAddress_(folly::none),
      repo_(repo),
      timeout_(timeout),
      executor_(executor),
      sslContext_(sslContext) {}

MononokeBackingStore::~MononokeBackingStore() {}

folly::Future<std::unique_ptr<Tree>> MononokeBackingStore::getTree(
    const Hash& id) {
  URL url(folly::sformat("/{}/tree/{}", repo_, id.toString()));

  return folly::via(executor_)
      .thenValue([this, url](auto&&) { return sendRequest(url); })
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return convertBufToTree(std::move(buf), id);
      });
}

folly::Future<std::unique_ptr<Blob>> MononokeBackingStore::getBlob(
    const Hash& id) {
  URL url(folly::sformat("/{}/blob/{}", repo_, id.toString()));
  return folly::via(executor_)
      .thenValue([this, url](auto&&) { return sendRequest(url); })
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return std::make_unique<Blob>(id, *buf);
      });
}

folly::Future<std::unique_ptr<Tree>> MononokeBackingStore::getTreeForCommit(
    const Hash& commitID) {
  URL url(folly::sformat("/{}/changeset/{}", repo_, commitID.toString()));
  return folly::via(executor_)
      .thenValue([this, url](auto&&) { return sendRequest(url); })
      .thenValue([&](std::unique_ptr<folly::IOBuf>&& buf) {
        auto s = buf->moveToFbString();
        auto parsed = folly::parseJson(s);
        auto hash = Hash(parsed.at("manifest").asString());
        return getTree(hash);
      });
}

folly::Future<folly::SocketAddress> MononokeBackingStore::getAddress(
    folly::EventBase* eventBase) {
  if (socketAddress_.hasValue()) {
    return folly::makeFuture(socketAddress_.value());
  }
  auto promise = folly::Promise<folly::SocketAddress>();
  auto future = promise.getFuture();

  auto& factory = servicerouter::cpp2::getClientFactory();
  auto selector = factory.getSelector();

  selector->getSelectionAsync(
      "mononoke-apiserver",
      servicerouter::DebugContext(),
      servicerouter::SelectionCacheCallback(
          [promise = std::move(promise)](
              const servicerouter::Selection& selection,
              servicerouter::DebugContext&& /* unused */) mutable {
            if (selection.hosts.empty()) {
              auto ex = make_exception_wrapper<std::runtime_error>(
                  std::string("no host found"));
              promise.setException(ex);
              return;
            }
            auto selected = folly::Random::rand32(selection.hosts.size());
            auto host = selection.hosts[selected];
            auto addr =
                folly::SocketAddress(host->getIpAddress(), host->getPort());
            promise.setValue(addr);
          }),
      eventBase,
      servicerouter::ServiceOptions(),
      servicerouter::ConnConfigs());

  return future;
}

folly::Future<std::unique_ptr<IOBuf>> MononokeBackingStore::sendRequest(
    const URL& url) {
  auto eventBase = folly::EventBaseManager::get()->getEventBase();

  return getAddress(eventBase).thenValue(
      [=](folly::SocketAddress addr) { return sendRequestImpl(addr, url); });
}

folly::Future<std::unique_ptr<IOBuf>> MononokeBackingStore::sendRequestImpl(
    folly::SocketAddress addr,
    const URL& url) {
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

  const folly::AsyncSocket::OptionMap opts{{{SOL_SOCKET, SO_REUSEADDR}, 1}};

  if (sslContext_ != nullptr) {
    connector->connectSSL(
        eventBase, addr, sslContext_, nullptr, timeout_, opts);
  } else {
    connector->connect(eventBase, addr, timeout_, opts);
  }

  /* capture `connector` to make sure it stays alive for the duration of the
     connection */
  return std::move(future).thenValue(
      [connector = std::move(connector), timer = std::move(timer)](
          std::unique_ptr<folly::IOBuf>&& buf) { return std::move(buf); });
}

} // namespace eden
} // namespace facebook
