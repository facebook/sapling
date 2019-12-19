/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/BackingStore.h"

#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/SSLOptions.h>
#include <optional>

namespace folly {
class IOBuf;
class Executor;
} // namespace folly

namespace proxygen {
class URL;
} // namespace proxygen

namespace facebook {
namespace eden {

class Blob;
class Hash;
class Tree;
class ServiceAddress;

using SocketAddressWithHostname = std::pair<folly::SocketAddress, std::string>;

/**
 * A BackingStore implementation that loads data out of a remote Mononoke
 * server over HTTP.
 */
class MononokeHttpBackingStore : public BackingStore {
 public:
  MononokeHttpBackingStore(
      std::unique_ptr<ServiceAddress> service,
      const std::string& repo,
      const std::chrono::milliseconds& timeout,
      folly::Executor* executor,
      const std::shared_ptr<folly::SSLContext> sslContext);

  virtual ~MononokeHttpBackingStore() override;

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  virtual folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;

 private:
  // Forbidden copy constructor and assignment operator
  MononokeHttpBackingStore(MononokeHttpBackingStore const&) = delete;
  MononokeHttpBackingStore& operator=(MononokeHttpBackingStore const&) = delete;

  folly::Future<SocketAddressWithHostname> getAddress();
  folly::Future<std::unique_ptr<folly::IOBuf>> sendRequest(
      folly::StringPiece endpoint,
      const Hash& id);
  folly::Future<std::unique_ptr<folly::IOBuf>> sendRequestImpl(
      SocketAddressWithHostname addr,
      folly::StringPiece endpoint,
      const Hash& id);

  std::unique_ptr<ServiceAddress> service_;
  std::string repo_;
  std::chrono::milliseconds timeout_;
  folly::Executor* executor_;
  std::shared_ptr<folly::SSLContext> sslContext_ = nullptr;
};
} // namespace eden
} // namespace facebook
