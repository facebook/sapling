/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakePrivHelper.h"

#include <folly/File.h>
#include <folly/futures/Future.h>
#include <utility>

#ifndef _WIN32
#include "eden/fs/testharness/FakeFuse.h"
#endif // _WIN32

using folly::File;
using folly::Future;
using folly::makeFuture;
using folly::Unit;
using std::runtime_error;
using std::string;

namespace facebook::eden {

#ifndef _WIN32

FakeFuseMountDelegate::FakeFuseMountDelegate(
    AbsolutePath mountPath,
    std::shared_ptr<FakeFuse> fuse) noexcept
    : mountPath_{std::move(mountPath)}, fuse_{std::move(fuse)} {}

folly::Future<folly::File> FakeFuseMountDelegate::fuseMount() {
  if (fuse_->isStarted()) {
    throwf<std::runtime_error>(
        "got request to create FUSE mount {}, "
        "but this mount is already running",
        mountPath_);
  }
  return fuse_->start();
}

folly::Future<folly::Unit> FakeFuseMountDelegate::fuseUnmount() {
  return folly::makeFutureWith([this] {
    wasFuseUnmountEverCalled_ = true;
    if (!fuse_->isStarted()) {
      throwf<std::runtime_error>(
          "got request to unmount {}, "
          "but this mount is not mounted",
          mountPath_);
    }
    return fuse_->close();
  });
}

bool FakeFuseMountDelegate::wasFuseUnmountEverCalled() const noexcept {
  return wasFuseUnmountEverCalled_;
}

FakePrivHelper::MountDelegate::~MountDelegate() = default;

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
  auto ret =
      mountDelegates_.emplace(mountPath.asString(), std::move(mountDelegate));
  if (!ret.second) {
    throwf<std::range_error>("mount {} already defined", mountPath);
  }
}

void FakePrivHelper::attachEventBase(folly::EventBase* /* eventBase */) {}

void FakePrivHelper::detachEventBase() {}

Future<File> FakePrivHelper::fuseMount(
    folly::StringPiece mountPath,
    bool /*readOnly*/) {
  return getMountDelegate(mountPath)->fuseMount();
}

Future<Unit> FakePrivHelper::nfsMount(
    folly::StringPiece /*mountPath*/,
    folly::SocketAddress /*mountdPort*/,
    folly::SocketAddress /*nfsdPort*/,
    bool /*readOnly*/,
    uint32_t /*iosize*/,
    bool /*useReaddirplus*/) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::nfsMount() not implemented"));
}

folly::Future<folly::Unit> FakePrivHelper::nfsUnmount(
    folly::StringPiece /*mountPath*/) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::nfsUnmount() not implemented"));
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

Future<Unit> FakePrivHelper::takeoverShutdown(
    folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::takeoverShutdown() not implemented"));
}

Future<Unit> FakePrivHelper::takeoverStartup(
    folly::StringPiece /* mountPath */,
    const std::vector<std::string>& /* bindMounts */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::takeoverStartup() not implemented"));
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
    throwf<std::range_error>(
        "got request to for FUSE mount {}, "
        "but no test FUSE endpoint defined for this path",
        mountPath);
  }
  return it->second;
}

folly::Future<folly::Unit> FakePrivHelper::setDaemonTimeout(
    std::chrono::nanoseconds /* duration */) {
  return folly::Unit{};
}

folly::Future<folly::Unit> FakePrivHelper::setUseEdenFs(bool /* useEdenFs */) {
  return folly::unit;
}
#endif // !_WIN32

} // namespace facebook::eden
