/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/futures/Future.h>
#include <chrono>
#include <memory>
#include <string>

#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/mononoke/CurlHttpClient.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class Executor;
} // namespace folly

namespace facebook {
namespace eden {

class Blob;
class Tree;
class Hash;
class ServiceAddress;

class MononokeCurlBackingStore : public BackingStore {
 public:
  MononokeCurlBackingStore(
      std::unique_ptr<ServiceAddress> service,
      AbsolutePath certificate,
      std::string repo,
      std::chrono::milliseconds timeout,
      std::shared_ptr<folly::Executor> executor);

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;

 private:
  std::string buildMononokePath(
      folly::StringPiece action,
      folly::StringPiece args);
  std::string repo_;
  std::unique_ptr<folly::Executor> clientThreadPool_;
  std::shared_ptr<folly::Executor> serverExecutor_;
};
} // namespace eden
} // namespace facebook
