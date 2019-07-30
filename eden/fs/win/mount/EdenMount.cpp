/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/win/mount/EdenMount.h"

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

static constexpr folly::StringPiece kEdenStracePrefix = "eden.strace.";

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
      dispatcher_{this},
      fsChannel_{config_->getMountPath(), this},
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

folly::Future<std::shared_ptr<const Tree>> EdenMount::getRootTreeFuture()
    const {
  auto commitHash = Hash{parentInfo_.rlock()->parents.parent1()};
  return objectStore_->getTreeForCommit(commitHash);
}

std::shared_ptr<const Tree> EdenMount::getRootTree() const {
  // TODO: We should convert callers of this API to use the Future-based
  // version.
  return getRootTreeFuture().get();
}

void EdenMount::start() {
  fsChannel_.start();
}

void EdenMount::stop() {
  fsChannel_.stop();
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

} // namespace eden
} // namespace facebook
