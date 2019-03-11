/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/mononoke/MononokeCurlBackingStore.h"

#include <folly/Executor.h>
#include <folly/json.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/mononoke/CurlHttpClient.h"
#include "eden/fs/store/mononoke/MononokeAPIUtils.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {
MononokeCurlBackingStore::MononokeCurlBackingStore(
    std::string host,
    AbsolutePath certificate,
    std::string repo,
    std::chrono::milliseconds timeout,
    std::shared_ptr<folly::Executor> executor)
    : conn_(CurlHttpClient(
          host,
          std::move(certificate),
          timeout,
          std::move(executor))),
      repo_(std::move(repo)) {}

folly::Future<std::unique_ptr<Tree>> MononokeCurlBackingStore::getTree(
    const Hash& id) {
  return conn_.futureGet(buildMononokePath("tree", id.toString()))
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return parseMononokeTree(std::move(buf), id);
      });
}

folly::Future<std::unique_ptr<Blob>> MononokeCurlBackingStore::getBlob(
    const Hash& id) {
  return conn_.futureGet(buildMononokePath("blob", id.toString()))
      .thenValue([id](std::unique_ptr<folly::IOBuf>&& buf) {
        return std::make_unique<Blob>(id, *buf);
      });
}

folly::Future<std::unique_ptr<Tree>> MononokeCurlBackingStore::getTreeForCommit(
    const Hash& commitID) {
  return conn_.futureGet(buildMononokePath("manifest", commitID.toString()))
      .thenValue([&](std::unique_ptr<folly::IOBuf>&& buf) {
        auto hash = Hash(
            folly::parseJson(buf->moveToFbString()).at("manifest").asString());
        return getTree(hash);
      });
}

std::string MononokeCurlBackingStore::buildMononokePath(
    folly::StringPiece action,
    folly::StringPiece args) {
  return folly::to<std::string>("/", repo_, "/", action, "/", args);
}
} // namespace eden
} // namespace facebook
