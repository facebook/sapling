/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/filter/HgSparseFilter.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/scm/lib/edenfs-ffi/src/lib.rs.h" // @manual

#include <folly/futures/Future.h>
#include <exception>
#include <memory>
#include <string>

namespace facebook::eden {

namespace {
std::string parseFilterId(folly::StringPiece filterId) {
  if (filterId == "null") {
    return filterId.str();
  }

  auto separatorIdx = filterId.find(":");
  auto commitId = hash20FromThrift(filterId.subpiece(separatorIdx + 1));
  auto filterIdStr =
      fmt::format("{}:{}", filterId.subpiece(0, separatorIdx), commitId);
  return filterIdStr;
}
} // namespace

ImmediateFuture<bool> HgSparseFilter::isPathFiltered(
    RelativePathPiece path,
    folly::StringPiece id) const {
  // We check if the filter is cached. If so, we can avoid fetching the Filter
  // Profile from Mercurial.
  auto parsedFilterId = parseFilterId(id);
  {
    // TODO(cuev): I purposely don't hold the lock after checking the cache.
    // This will lead to multiple threads adding to the cache, but it should be
    // faster overall? This should be a one time occurrence per FilterId.
    auto profiles = profiles_->rlock();
    auto profileIt = profiles->find(parsedFilterId);
    profiles.unlock();
    if (profileIt != profiles->end()) {
      return ImmediateFuture<bool>(
          profileIt->second->is_path_excluded(path.asString()));
    }
  }
  XLOGF(DBG8, "New filter id {}. Fetching from Mercurial.", id);

  auto filterId = rust::Str{parsedFilterId.data(), parsedFilterId.size()};
  auto pathToMount =
      rust::Str{checkoutPath_.view().data(), checkoutPath_.view().size()};
  auto [promise, rootFuture] =
      folly::makePromiseContract<rust::Box<SparseProfileRoot>>();
  auto rootPromise = std::make_shared<RootPromise>(std::move(promise));
  profile_from_filter_id(filterId, pathToMount, std::move(rootPromise));

  return ImmediateFuture{
      std::move(rootFuture)
          .deferValue(
              [filterId = std::move(parsedFilterId),
               path = path.copy(),
               profilesLock = profiles_](rust::Box<SparseProfileRoot>&& res) {
                auto profiles = profilesLock->wlock();
                auto [profileIt, _] =
                    profiles->try_emplace(filterId, std::move(res));
                return profileIt->second->is_path_excluded(path.asString());
              })};
}

} // namespace facebook::eden
