/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/utils/PathFuncs.h"

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
  FakePrivHelper();

  void registerMount(
      AbsolutePathPiece mountPath,
      std::shared_ptr<FakeFuse> fuse);

  // PrivHelper functions
  void start(folly::EventBase* eventBase) override;
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
  int stop() override;

 private:
  FakePrivHelper(FakePrivHelper const&) = delete;
  FakePrivHelper& operator=(FakePrivHelper const&) = delete;

  std::unordered_map<std::string, std::shared_ptr<FakeFuse>> mounts_;
};
} // namespace eden
} // namespace facebook
