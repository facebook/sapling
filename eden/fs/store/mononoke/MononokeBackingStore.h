/*
 *  Copyright (c) 2017-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/store/BackingStore.h"

#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>

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

/**
 * A BackingStore implementation that loads data out of a remote Mononoke
 * server over HTTP.
 */
class MononokeBackingStore : public BackingStore {
 public:
  MononokeBackingStore(
      const folly::SocketAddress& sa,
      const std::string& repo,
      const std::chrono::milliseconds& timeout,
      folly::Executor* executor);
  virtual ~MononokeBackingStore();

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;

 private:
  // Forbidden copy constructor and assignment operator
  MononokeBackingStore(MononokeBackingStore const&) = delete;
  MononokeBackingStore& operator=(MononokeBackingStore const&) = delete;

  folly::Future<std::unique_ptr<folly::IOBuf>> sendRequest(
      const proxygen::URL& url);

  folly::SocketAddress sa_;
  std::string repo_;
  std::chrono::milliseconds timeout_;
  folly::Executor* executor_;
};
} // namespace eden
} // namespace facebook
