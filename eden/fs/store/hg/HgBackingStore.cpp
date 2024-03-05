/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/hg/HgBackingStore.h"

#include <memory>

#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/Try.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/Throw.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/eden-config.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/StoreResult.h"
#include "eden/fs/store/hg/HgDatapackStore.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgProxyHash.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/StructuredLogger.h"

using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::makeSemiFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;

namespace facebook::eden {

HgBackingStore::HgBackingStore(
    folly::Executor* retryThreadPool,
    std::shared_ptr<LocalStore> localStore,
    HgDatapackStore* datapackStore,
    UnboundedQueueExecutor* serverThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    EdenStatsPtr stats,
    std::shared_ptr<StructuredLogger> logger)
    : localStore_(std::move(localStore)),
      stats_(stats.copy()),
      retryThreadPool_(retryThreadPool),
      config_(std::move(config)),
      serverThreadPool_(serverThreadPool),
      logger_(std::move(logger)),
      datapackStore_(datapackStore) {}

/**
 * Create an HgBackingStore suitable for use in unit tests. It uses an inline
 * executor to process loaded objects rather than the thread pools used in
 * production Eden.
 */
HgBackingStore::HgBackingStore(
    folly::Executor* retryThreadPool,
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<LocalStore> localStore,
    HgDatapackStore* datapackStore,
    EdenStatsPtr stats)
    : localStore_{std::move(localStore)},
      stats_{std::move(stats)},
      retryThreadPool_{retryThreadPool},
      config_(std::move(config)),
      serverThreadPool_{retryThreadPool_},
      logger_(nullptr),
      datapackStore_(datapackStore) {}

HgBackingStore::~HgBackingStore() = default;

} // namespace facebook::eden
