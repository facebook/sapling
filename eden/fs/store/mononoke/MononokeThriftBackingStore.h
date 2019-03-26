/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <scm/mononoke/apiserver/gen-cpp2/MononokeAPIServiceAsyncClient.h>

#include "eden/fs/store/BackingStore.h"

namespace facebook {
namespace eden {
class MononokeThriftBackingStore : public BackingStore {
 public:
  MononokeThriftBackingStore(
      std::string tierName,
      std::string repo,
      std::shared_ptr<folly::Executor> executor);
  MononokeThriftBackingStore(
      std::unique_ptr<
          scm::mononoke::apiserver::thrift::MononokeAPIServiceAsyncClient>
          client,
      std::string repo,
      std::shared_ptr<folly::Executor> executor);

  virtual ~MononokeThriftBackingStore() override;

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;

 private:
  // Forbidden move constructor and assignment operator
  MononokeThriftBackingStore(MononokeThriftBackingStore&&) = delete;
  MononokeThriftBackingStore& operator=(MononokeThriftBackingStore&&) = delete;

  std::string repo_;
  std::unique_ptr<
      scm::mononoke::apiserver::thrift::MononokeAPIServiceAsyncClient>
      client_;
  std::shared_ptr<folly::Executor> executor_;
};
} // namespace eden
} // namespace facebook
