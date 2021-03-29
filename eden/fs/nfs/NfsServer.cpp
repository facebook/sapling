/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsServer.h"
#include <folly/concurrency/DynamicBoundedQueue.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include "eden/fs/nfs/Nfsd3.h"

namespace facebook::eden {

namespace {
using Task = folly::CPUThreadPoolExecutor::CPUTask;
using Queue = folly::DMPMCQueue<Task, true>;
/**
 * Task queue that will hold the pending NFS requests.
 *
 * This is backed by a DMPMCQueue.
 */
class NfsTaskQueue : public folly::BlockingQueue<Task> {
 public:
  explicit NfsTaskQueue(uint64_t maxInflightRequests)
      : queue_(Queue{maxInflightRequests}) {}

  folly::BlockingQueueAddResult add(Task item) override {
    queue_.enqueue(std::move(item));
    return sem_.post();
  }

  Task take() override {
    sem_.wait();
    Task res;
    queue_.dequeue(res);
    return res;
  }

  folly::Optional<Task> try_take_for(std::chrono::milliseconds time) override {
    if (!sem_.try_wait_for(time)) {
      return folly::none;
    }
    Task res;
    queue_.dequeue(res);
    return res;
  }

  size_t size() override {
    return queue_.size();
  }

 private:
  folly::LifoSem sem_;
  Queue queue_;
};
} // namespace

NfsServer::NfsServer(
    bool registerMountdWithRpcbind,
    folly::EventBase* evb,
    uint64_t numServicingThreads,
    uint64_t maxInflightRequests)
    : evb_(evb),
      threadPool_(std::make_shared<folly::CPUThreadPoolExecutor>(
          numServicingThreads,
          std::make_unique<NfsTaskQueue>(maxInflightRequests),
          std::make_unique<folly::NamedThreadFactory>("NfsThreadPool"))),
      mountd_(registerMountdWithRpcbind, evb_, threadPool_) {}

NfsServer::NfsMountInfo NfsServer::registerMount(
    AbsolutePathPiece path,
    InodeNumber rootIno,
    std::unique_ptr<NfsDispatcher> dispatcher,
    const folly::Logger* straceLogger,
    std::shared_ptr<ProcessNameCache> processNameCache,
    folly::Duration requestTimeout,
    Notifications* FOLLY_NULLABLE notifications,
    bool caseSensitive) {
  auto nfsd = std::make_unique<Nfsd3>(
      false,
      evb_,
      threadPool_,
      std::move(dispatcher),
      straceLogger,
      std::move(processNameCache),
      requestTimeout,
      notifications,
      caseSensitive);
  mountd_.registerMount(path, rootIno);

  auto nfsdPort = nfsd->getPort();
  return {std::move(nfsd), mountd_.getPort(), nfsdPort};
}

void NfsServer::unregisterMount(AbsolutePathPiece path) {
  mountd_.unregisterMount(path);
}

} // namespace facebook::eden

#endif
