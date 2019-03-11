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

class MononokeCurlBackingStore : public BackingStore {
 public:
  MononokeCurlBackingStore(
      std::string host,
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
  CurlHttpClient conn_;
  std::string repo_;
};
} // namespace eden
} // namespace facebook
