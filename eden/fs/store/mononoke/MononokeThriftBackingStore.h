/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/store/BackingStore.h"

namespace scm::mononoke::apiserver::thrift {
class MononokeAPIServiceAsyncClient;
}

namespace facebook {
namespace eden {
class MononokeThriftBackingStore : public BackingStore {
 public:
  MononokeThriftBackingStore(
      std::string tierName,
      std::string repo,
      folly::Executor* executor);

  MononokeThriftBackingStore(
      std::unique_ptr<
          scm::mononoke::apiserver::thrift::MononokeAPIServiceAsyncClient>
          testClient,
      std::string repo,
      folly::Executor* executor);

  virtual ~MononokeThriftBackingStore() override;

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  virtual folly::SemiFuture<std::unique_ptr<Blob>> getBlob(
      const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;

 private:
  // Forbidden move constructor and assignment operator
  MononokeThriftBackingStore(MononokeThriftBackingStore&&) = delete;
  MononokeThriftBackingStore& operator=(MononokeThriftBackingStore&&) = delete;

  template <typename Func>
  std::invoke_result_t<
      Func,
      scm::mononoke::apiserver::thrift::MononokeAPIServiceAsyncClient*>
  withClient(Func&& func);

  std::string serviceName_;
  std::string repo_;
  folly::Executor* executor_;

  std::unique_ptr<
      scm::mononoke::apiserver::thrift::MononokeAPIServiceAsyncClient>
      testClient_;
};
} // namespace eden
} // namespace facebook
