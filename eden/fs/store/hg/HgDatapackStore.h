/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Promise.h>
#include <optional>
#include <string_view>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/BlobFwd.h"
#include "eden/fs/model/BlobMetadataFwd.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/hg/HgBackingStoreOptions.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/scm/lib/backingstore/include/SaplingNativeBackingStore.h"

namespace facebook::eden {

class Hash20;
class HgProxyHash;
class HgImportRequest;
class ObjectId;
class ReloadableConfig;
class StructuredLogger;
class FaultInjector;
template <typename T>
class RefPtr;
class ObjectFetchContext;
using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

class HgDatapackStore {
 public:
  using ImportRequestsList = std::vector<std::shared_ptr<HgImportRequest>>;
  using SaplingNativeOptions = sapling::SaplingNativeBackingStoreOptions;

  /**
   * FaultInjector must be valid for the lifetime of the HgDatapackStore.
   * Currently, FaultInjector is one of the last things destructed when Eden
   * shutsdown. Likely we should use shared pointers instead of raw pointers
   * for FaultInjector though. TODO: T171327256.
   */
  HgDatapackStore(
      sapling::SaplingNativeBackingStore* store,
      HgBackingStoreOptions* runtimeOptions,
      std::shared_ptr<ReloadableConfig> config,
      std::shared_ptr<StructuredLogger> logger,
      FaultInjector* FOLLY_NONNULL faultInjector)
      : store_{store},
        runtimeOptions_{runtimeOptions},
        config_{std::move(config)},
        logger_{std::move(logger)},
        faultInjector_{*faultInjector} {}

  using ImportRequestsMap = std::
      map<sapling::NodeId, std::pair<ImportRequestsList, RequestMetricsScope>>;

  // Raw pointer to the `std::unique_ptr<sapling::SaplingNativeBackingStore>`
  // owned by the same `HgQueuedBackingStore` that also has a `std::unique_ptr`
  // to this class. Holding this raw pointer is safe because this class's
  // lifetime is controlled by the same class (`HgQueuedBackingStore`) that
  // controls the lifetime of the underlying
  // `sapling::SaplingNativeBackingStore` here
  sapling::SaplingNativeBackingStore* store_;

  // Raw pointer to the `std::unique_ptr<HgBackingStoreOptions>` owned
  // by the same `HgQueuedBackingStore` that also has a `std::unique_ptr` to
  // this class. Holding this raw pointer is safe because this class's lifetime
  // is controlled by the same class (`HgQueuedBackingStore`) that controls the
  // lifetime of the underlying `HgBackingStoreOptions` here
  HgBackingStoreOptions* runtimeOptions_;
  std::shared_ptr<ReloadableConfig> config_;
  std::shared_ptr<StructuredLogger> logger_;
  FaultInjector& faultInjector_;

  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedBlobWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList liveBatchedTreeWatches_;
  mutable RequestMetricsScope::LockedRequestWatchList
      liveBatchedBlobMetaWatches_;
};

} // namespace facebook::eden
