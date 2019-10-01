/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/store/hg/HgDatapackStore.h"

#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include <memory>
#include <optional>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/hg/HgProxyHash.h"

#include "scm/hg/lib/configparser/ConfigParser.h" // @manual

namespace facebook {
namespace eden {
HgDatapackStore::HgDatapackStore(
    AbsolutePathPiece repository,
    folly::StringPiece repoName,
    AbsolutePathPiece cachePath,
    RelativePathPiece subdir) {
  std::vector<AbsolutePath> paths;

  paths.emplace_back(repository + ".hg/store"_relpath + subdir);
  paths.emplace_back(cachePath + RelativePathPiece{repoName} + subdir);

  std::vector<const char*> cStrings;
  for (auto& path : paths) {
    cStrings.emplace_back(path.c_str());
  }

  store_ = folly::Synchronized<DataPackUnion>(
      DataPackUnion(cStrings.data(), cStrings.size()));
}

std::unique_ptr<Blob> HgDatapackStore::getBlob(
    const Hash& id,
    const HgProxyHash& hgInfo) {
  auto store = store_.value().wlock();

  try {
    auto content =
        store->get(hgInfo.path().stringPiece(), hgInfo.revHash().getBytes());
    if (content) {
      auto bytes = content->bytes();
      return std::make_unique<Blob>(
          id,
          folly::IOBuf(
              folly::IOBuf::CopyBufferOp{}, bytes.data(), bytes.size()));
    }
    // If we get here, it was a KeyError, meaning that the data wasn't
    // present in the hgcache, rather than a more terminal problems such
    // as an IOError of some kind.
    // Regardless, we'll return nullptr and fallback to other sources.
  } catch (const DataPackUnionGetError& exc) {
    XLOG(ERR) << "Error getting " << hgInfo.path() << " " << hgInfo.revHash()
              << " from dataPackStore_: " << exc.what()
              << ", will fall back to other methods";
  }

  return nullptr;
}

std::optional<HgDatapackStore> makeHgDatapackStore(
    AbsolutePathPiece repository,
    std::shared_ptr<ReloadableConfig> edenConfig) {
  HgRcConfigSet config;

  auto repoConfigPath = repository + ".hg/hgrc"_relpath;

  try {
    config.loadSystem();
    config.loadUser();
    config.loadPath(repoConfigPath.c_str());
  } catch (const HgRcConfigError& exc) {
    XLOG(ERR)
        << "Disabling loading blobs from hgcache: Error(s) while loading '"
        << repoConfigPath << "': " << exc.what();
    return std::nullopt;
  }

  auto maybeRepoName = config.get("remotefilelog", "reponame");
  auto maybeCachePath = config.get("remotefilelog", "cachepath");

  if (maybeRepoName.hasValue() && maybeCachePath.hasValue()) {
    folly::StringPiece repoName{maybeRepoName.value().bytes()};

    std::optional<folly::StringPiece> homeDir = edenConfig
        ? std::make_optional(
              edenConfig->getEdenConfig()->getUserHomePath().stringPiece())
        : std::nullopt;
    auto cachePath =
        expandUser(folly::StringPiece{maybeCachePath.value().bytes()}, homeDir);

    return std::make_optional<HgDatapackStore>(
        repository, repoName, cachePath, "packs"_relpath);

    // TODO: create a treePackStore here with `packs/manifests` as the subdir.
    // That depends on some future work to port the manifest code from C++
    // to Rust.
  } else {
    XLOG(DBG2)
        << "Disabling loading blobs from hgcache: remotefilelog.reponame "
           "and/or remotefilelog.cachepath are not configured";
  }
  return std::nullopt;
}
} // namespace eden
} // namespace facebook
