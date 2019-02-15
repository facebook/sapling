/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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

namespace {
class FakeFuseMountDelegate : public FakePrivHelper::MountDelegate {
 public:
  explicit FakeFuseMountDelegate(
      AbsolutePath mountPath,
      std::shared_ptr<FakeFuse> fuse) noexcept
      : mountPath_{std::move(mountPath)}, fuse_{std::move(fuse)} {}

  folly::Future<folly::File> fuseMount() override {
    if (fuse_->isStarted()) {
      throw std::runtime_error(folly::to<string>(
          "got request to create FUSE mount ",
          mountPath_,
          ", but this mount is already running"));
    }
    return fuse_->start();
  }

 private:
  AbsolutePath mountPath_;
  std::shared_ptr<FakeFuse> fuse_;
};
} // namespace

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
  auto it = mountDelegates_.find(mountPath.str());
  if (it == mountDelegates_.end()) {
    throw std::range_error(folly::to<string>(
        "got request to create FUSE mount ",
        mountPath,
        ", but no test FUSE endpoint defined for this path"));
  }
  return it->second->fuseMount();
}

Future<Unit> FakePrivHelper::fuseUnmount(folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::fuseUnmount() not implemented"));
}

Future<Unit> FakePrivHelper::bindMount(
    folly::StringPiece /* clientPath */,
    folly::StringPiece /* mountPath */) {
  return makeFuture<Unit>(
      runtime_error("FakePrivHelper::bindMount() not implemented"));
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

} // namespace eden
} // namespace facebook
