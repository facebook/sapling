/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsServer.h"

#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include "eden/fs/nfs/Nfsd3.h"
#include "eden/fs/utils/EdenTaskQueue.h"

namespace facebook::eden {

NfsServer::NfsServer(
    folly::EventBase* evb,
    uint64_t numServicingThreads,
    uint64_t maxInflightRequests,
    const std::shared_ptr<StructuredLogger>& structuredLogger)
    : evb_(evb),
      threadPool_(std::make_shared<folly::CPUThreadPoolExecutor>(
          numServicingThreads,
          std::make_unique<EdenTaskQueue>(maxInflightRequests),
          std::make_unique<folly::NamedThreadFactory>("NfsThreadPool"))),
      mountd_(evb_, threadPool_, structuredLogger) {}

void NfsServer::initialize(
    folly::SocketAddress addr,
    bool registerMountdWithRpcbind) {
  mountd_.initialize(addr, registerMountdWithRpcbind);
}

void NfsServer::initialize(folly::File&& socket) {
  mountd_.initialize(std::move(socket));
}

NfsServer::NfsMountInfo NfsServer::registerMount(
    AbsolutePathPiece path,
    InodeNumber rootIno,
    std::unique_ptr<NfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> processNameCache,
    std::shared_ptr<FsEventLogger> fsEventLogger,
    const std::shared_ptr<StructuredLogger>& structuredLogger,
    folly::Duration requestTimeout,
    std::shared_ptr<Notifier> notifier,
    CaseSensitivity caseSensitive,
    uint32_t iosize,
    size_t traceBusCapacity) {
  auto nfsd = std::make_unique<Nfsd3>(
      evb_,
      threadPool_,
      std::move(dispatcher),
      straceLogger,
      std::move(processNameCache),
      std::move(fsEventLogger),
      structuredLogger,
      requestTimeout,
      std::move(notifier),
      caseSensitive,
      iosize,
      traceBusCapacity);
  mountd_.registerMount(path, rootIno);

  return {std::move(nfsd), mountd_.getAddr()};
}

void NfsServer::unregisterMount(AbsolutePathPiece path) {
  mountd_.unregisterMount(path);
}

folly::SemiFuture<folly::File> NfsServer::takeoverStop() {
  return mountd_.takeoverStop();
}

} // namespace facebook::eden

#endif
