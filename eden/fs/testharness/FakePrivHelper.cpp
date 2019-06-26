/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/FakePrivHelper.h"

#include <folly/Conv.h>
#include <folly/File.h>
#include <folly/futures/Future.h>
#include <utility>
#include "eden/fs/testharness/FakeFuse.h"

using folly::File;
using folly::Future;
using folly::makeFuture;
using folly::Unit;
using std::runtime_error;
using std::string;

namespace facebook {
namespace eden {

FakeFuseMountDelegate::FakeFuseMountDelegate(
    AbsolutePath mountPath,
    std::shared_ptr<FakeFuse> fuse) noexcept
    : mountPath_{std::move(mountPath)}, fuse_{std::move(fuse)} {}

folly::Future<folly::File> FakeFuseMountDelegate::fuseMount() {
  if (fuse_->isStarted()) {
    throw std::runtime_error(folly::to<string>(
        "got request to create FUSE mount ",
        mountPath_,
        ", but this mount is already running"));
  }
  return fuse_->start();
}

folly::Future<folly::Unit> FakeFuseMountDelegate::fuseUnmount() {
  return folly::makeFutureWith([this] {
    wasFuseUnmountEverCalled_ = true;
    if (!fuse_->isStarted()) {
      throw std::runtime_error(folly::to<string>(
          "got request to unmount ",
          mountPath_,
          ", but this mount is not moutned"));
    }
    return fuse_->close();
  });
}

bool FakeFuseMountDelegate::wasFuseUnmountEverCalled() const noexcept {
  return wasFuseUnmountEverCalled_;
}

FakePrivHelper::MountDelegate::~MountDelegate() = default;

FakePrivHelper::FakePrivHelper() {}

void FakePrivHelper::registerMount(
    AbsolutePathPiece mountPath,
    std::shared_ptr<FakeFuse> fuse) {
  registerMountDelegate(
      mountPath,
      std::make_shared<FakeFuseMountDelegate>(
          AbsolutePath{mountPath}, std::move(fuse)));
}

void FakePrivHelper::registerMountDelegate(
    AbsolutePathPiece mountPath,
    std::shared_ptr<MountDelegate> mountDelegate) {
  auto ret = mountDelegates_.emplace(
      mountPath.stringPiece().str(), std::move(mountDelegate));
  if (!ret.second) {
    throw std::range_error(
        folly::to<string>("mount ", mountPath, " already defined"));
  }
}

void FakePrivHelper::attachEventBase(folly::EventBase* /* eventBase */) {}

void FakePrivHelper::detachEventBase() {}

Future<File> FakePrivHelper::fuseMount(folly::StringPiece mountPath) {
  return getMountDelegate(mountPath)->fuseMount();
}

Future<Unit> FakePrivHelper::fuseUnmount(folly::StringPiece mountPath) {
  return folly::makeFutureWith(
      [&] { return getMountDelegate(mountPath)->fuseUnmount(); });
}

Future<Unit> FakePrivHelper::bindMount(
    folly::StringPiece /* clientPath */,
    folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::bindMount() not implemented"));
}

folly::Future<folly::Unit> FakePrivHelper::bindUnMount(
    folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::bindUnMount() not implemented"));
}

Future<Unit> FakePrivHelper::fuseTakeoverShutdown(
    folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::fuseTakeoverShutdown() not implemented"));
}

Future<Unit> FakePrivHelper::fuseTakeoverStartup(
    folly::StringPiece /* mountPath */,
    const std::vector<std::string>& /* bindMounts */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::fuseTakeoverStartup() not implemented"));
}

Future<Unit> FakePrivHelper::setLogFile(folly::File /* logFile */) {
  return makeFuture();
}

int FakePrivHelper::stop() {
  return 0;
}

std::shared_ptr<FakePrivHelper::MountDelegate> FakePrivHelper::getMountDelegate(
    folly::StringPiece mountPath) {
  auto it = mountDelegates_.find(mountPath.str());
  if (it == mountDelegates_.end()) {
    throw std::range_error(folly::to<string>(
        "got request to for FUSE mount ",
        mountPath,
        ", but no test FUSE endpoint defined for this path"));
  }
  return it->second;
}

} // namespace eden
} // namespace facebook
