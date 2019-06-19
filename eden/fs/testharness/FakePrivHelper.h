/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/futures/Future.h>
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/utils/PathFuncs.h"

#include <memory>
#include <string>
#include <unordered_map>

namespace facebook {
namespace eden {

class FakeFuse;

/**
 * FakePrivHelper implements the PrivHelper API, but returns FakeFuse
 * connections rather than performing actual FUSE mounts to the kernel.
 *
 * This allows test code to directly control the FUSE messages sent to an
 * EdenMount.
 */
class FakePrivHelper : public PrivHelper {
 public:
  class MountDelegate {
   public:
    virtual ~MountDelegate();

    virtual folly::Future<folly::File> fuseMount() = 0;
    virtual folly::Future<folly::Unit> fuseUnmount() = 0;
  };

  FakePrivHelper();

  void registerMount(
      AbsolutePathPiece mountPath,
      std::shared_ptr<FakeFuse> fuse);

  void registerMountDelegate(
      AbsolutePathPiece mountPath,
      std::shared_ptr<MountDelegate>);

  // PrivHelper functions
  void attachEventBase(folly::EventBase* eventBase) override;
  void detachEventBase() override;
  folly::Future<folly::File> fuseMount(folly::StringPiece mountPath) override;
  folly::Future<folly::Unit> fuseUnmount(folly::StringPiece mountPath) override;
  folly::Future<folly::Unit> bindMount(
      folly::StringPiece clientPath,
      folly::StringPiece mountPath) override;
  folly::Future<folly::Unit> fuseTakeoverShutdown(
      folly::StringPiece mountPath) override;
  folly::Future<folly::Unit> fuseTakeoverStartup(
      folly::StringPiece mountPath,
      const std::vector<std::string>& bindMounts) override;
  folly::Future<folly::Unit> setLogFile(folly::File logFile) override;
  int stop() override;

 private:
  FakePrivHelper(FakePrivHelper const&) = delete;
  FakePrivHelper& operator=(FakePrivHelper const&) = delete;

  std::shared_ptr<MountDelegate> getMountDelegate(folly::StringPiece mountPath);

  std::unordered_map<std::string, std::shared_ptr<MountDelegate>>
      mountDelegates_;
};

class FakeFuseMountDelegate : public FakePrivHelper::MountDelegate {
 public:
  explicit FakeFuseMountDelegate(
      AbsolutePath mountPath,
      std::shared_ptr<FakeFuse>) noexcept;

  folly::Future<folly::File> fuseMount() override;
  folly::Future<folly::Unit> fuseUnmount() override;

  FOLLY_NODISCARD bool wasFuseUnmountEverCalled() const noexcept;

 private:
  AbsolutePath mountPath_;
  std::shared_ptr<FakeFuse> fuse_;
  bool wasFuseUnmountEverCalled_{false};
};
} // namespace eden
} // namespace facebook
