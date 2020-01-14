/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/mount/EdenMount.h"

#include <thrift/lib/cpp2/async/ResponseChannel.h>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"
#include "eden/fs/win/mount/CurrentState.h"
#include "eden/fs/win/mount/GenerateStatus.h"
#include "eden/fs/win/mount/RepoConfig.h"

#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

static constexpr folly::StringPiece kEdenStracePrefix = "eden.strace.";
constexpr std::wstring_view kCurrentStateDataPath =
    L"SOFTWARE\\facebook\\eden\\repo";

static uint64_t generateLuid() {
  LUID luid;
  if (AllocateLocallyUniqueId(&luid)) {
    uint64_t id = luid.HighPart;
    return id << 32 | luid.LowPart;
  }
  throw std::system_error(
      GetLastError(), Win32ErrorCategory::get(), "Failed to generate the luid");
}

std::shared_ptr<EdenMount> EdenMount::create(
    std::unique_ptr<CheckoutConfig> config,
    std::shared_ptr<ObjectStore> objectStore,
    std::shared_ptr<ServerState> serverState,
    std::unique_ptr<Journal> journal) {
  return std::shared_ptr<EdenMount>(
      new EdenMount(
          std::move(config),
          std::move(objectStore),
          std::move(serverState),
          std::move(journal)),
      EdenMountDeleter{});
}

EdenMount::EdenMount(
    std::unique_ptr<CheckoutConfig> config,
    std::shared_ptr<ObjectStore> objectStore,
    std::shared_ptr<ServerState> serverState,
    std::unique_ptr<Journal> journal)
    : config_{std::move(config)},
      serverState_{std::move(serverState)},
      objectStore_{std::move(objectStore)},
      straceLogger_{kEdenStracePrefix.str() + config_->getMountPath().value()},
      journal_{std::move(journal)},
      mountGeneration_{generateLuid()} {
  auto parents = std::make_shared<ParentCommits>(config_->getParentCommits());

  XLOGF(
      INFO,
      "Creating eden mount {} Parent Commit {}",
      getPath(),
      parents->parent1().toString());
  parentInfo_.wlock()->parents.setParents(*parents);
}

EdenMount::~EdenMount() {}

const AbsolutePath& EdenMount::getPath() const {
  return config_->getMountPath();
}

folly::Future<std::shared_ptr<const Tree>> EdenMount::getRootTree() const {
  auto commitHash = Hash{parentInfo_.rlock()->parents.parent1()};
  return objectStore_->getTreeForCommit(commitHash);
}

folly::Future<folly::Unit> EdenMount::diff(
    DiffCallback* callback,
    Hash commitHash,
    bool listIgnored,
    bool enforceCurrentParent,
    apache::thrift::ResponseChannelRequest* request) const {
  if (enforceCurrentParent) {
    auto parentInfo = parentInfo_.rlock(std::chrono::milliseconds{500});

    if (!parentInfo) {
      // We failed to get the lock, which generally means a checkout is in
      // progress.
      return folly::makeFuture<folly::Unit>(newEdenError(
          EdenErrorType::CHECKOUT_IN_PROGRESS,
          "cannot compute status while a checkout is currently in progress"));
    }

    if (parentInfo->parents.parent1() != commitHash) {
      return folly::makeFuture<folly::Unit>(newEdenError(
          EdenErrorType::OUT_OF_DATE_PARENT,
          "error computing status: requested parent commit is out-of-date: requested ",
          commitHash,
          ", but current parent commit is ",
          parentInfo->parents.parent1(),
          ".\nTry running `eden doctor` to remediate"));
    }

    // TODO: Should we perhaps hold the parentInfo read-lock for the duration of
    // the status operation?  This would block new checkout operations from
    // starting until we have finished computing this status call.
  }

  return objectStore_->getTreeForCommit(commitHash)
      .thenValue(
          [this, callback, request](std::shared_ptr<const Tree>&& rootTree) {
            GenerateStatus generator(
                getObjectStore(),
                getCurrentState(),
                std::move(edenToWinPath(getPath().value())),
                callback,
                request);
            return generator.compute(rootTree).ensure(
                [generator = std::move(generator)]() {});
          });
}

folly::Future<std::unique_ptr<ScmStatus>> EdenMount::diff(
    Hash commitHash,
    bool listIgnored,
    bool enforceCurrentParent,
    apache::thrift::ResponseChannelRequest* FOLLY_NULLABLE request) {
  auto callback = std::make_unique<ScmStatusDiffCallback>();
  auto callbackPtr = callback.get();
  return this
      ->diff(
          callbackPtr, commitHash, listIgnored, enforceCurrentParent, request)
      .thenValue([callback = std::move(callback)](auto&&) {
        return std::make_unique<ScmStatus>(
            std::move(callback->extractStatus()));
      });
}

void EdenMount::start() {
  fsChannel_->start();
  createRepoConfig(
      getPath(), serverState_->getSocketPath(), config_->getClientDirectory());
  if (!getCurrentState()) {
    currentState_ = std::make_unique<CurrentState>(
        kCurrentStateDataPath,
        multibyteToWideString(getMountId(getPath().c_str())));
  }
}

void EdenMount::stop() {
  fsChannel_->stop();
}

void EdenMount::destroy() {
  XLOGF(
      INFO, "Destroying EdenMount (0x{:x})", reinterpret_cast<uintptr_t>(this));

  auto oldState = state_.exchange(State::DESTROYING);
  switch (oldState) {
    case State::RUNNING:
      stop();
      break;
  }
  delete this;
}

void EdenMount::resetParents(const ParentCommits& parents) {
  // Hold the snapshot lock around the entire operation.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  XLOG(DBG1) << "resetting snapshot for " << this->getPath() << " from "
             << oldParents << " to " << parents;

  config_->setParentCommits(parents);
  parentsLock->parents.setParents(parents);

  journal_->recordHashUpdate(oldParents.parent1(), parents.parent1());
}

void EdenMount::resetParent(const Hash& parent) {
  resetParents(ParentCommits{parent});
}

} // namespace eden
} // namespace facebook
