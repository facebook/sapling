/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/filter/HgSparseFilter.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/filter/Filter.h"
#include "eden/scm/lib/edenfs_ffi/include/ffi.h"
#include "eden/scm/lib/edenfs_ffi/src/lib.rs.h" // @manual

#include <folly/futures/Future.h>
#include <exception>
#include <memory>
#include <stdexcept>
#include <string>

namespace facebook::eden {

namespace {
FilterCoverage determineFilterCoverage(
    const rust::Box<facebook::eden::MercurialMatcher>& matcher,
    std::string_view path) {
  auto rustPath = rust::Str{path.data(), path.size()};
  auto res = matcher->is_recursively_unfiltered(rustPath);
  switch (res) {
    case FilterDirectoryMatch::RecursivelyUnfiltered:
      return FilterCoverage::RECURSIVELY_UNFILTERED;
    case FilterDirectoryMatch::RecursivelyFiltered:
      return FilterCoverage::RECURSIVELY_FILTERED;
    case FilterDirectoryMatch::Unfiltered:
      return FilterCoverage::UNFILTERED;
    default:
      throwf<std::invalid_argument>(
          "Rust returned an invalid filter FilterDirectoryMatch result: {}",
          static_cast<uint8_t>(res));
  }
}
} // namespace

ImmediateFuture<FilterCoverage> HgSparseFilter::getFilterCoverageForPath(
    RelativePathPiece path,
    folly::StringPiece id) const {
  // If filterId is "null", Mercurial is reporting that no filters are active
  if (id == kNullFilterId) {
    return FilterCoverage::RECURSIVELY_UNFILTERED;
  }

  // We check if the filter is cached. If so, we can avoid fetching the Filter
  // Profile from Mercurial.
  {
    auto profiles = profiles_->rlock();
    auto profileIt = profiles->find(id);
    profiles.unlock();
    if (profileIt != profiles->end()) {
      return ImmediateFuture<FilterCoverage>{
          determineFilterCoverage(profileIt->second, path.view())};
    }
  }
  XLOGF(DBG8, "New filter id {}. Fetching from Mercurial.", id);

  auto filterId = rust::Str{id.data(), id.size()};
  auto pathToMount =
      rust::Str{checkoutPath_.view().data(), checkoutPath_.view().size()};
  auto [promise, rootFuture] =
      folly::makePromiseContract<rust::Box<MercurialMatcher>>();
  auto rootPromise = std::make_unique<MatcherPromise>(std::move(promise));
  profile_from_filter_id(filterId, pathToMount, std::move(rootPromise));

  return ImmediateFuture{
      std::move(rootFuture)
          .deferValue(
              [filterId = id.toString(),
               path = path.copy(),
               profilesLock = profiles_](rust::Box<MercurialMatcher>&& res) {
                auto profiles = profilesLock->wlock();
                auto [profileIt, _] =
                    profiles->try_emplace(filterId, std::move(res));
                profiles.unlock();
                return determineFilterCoverage(profileIt->second, path.view());
              })};
}

} // namespace facebook::eden
