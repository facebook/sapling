/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/ThriftGlobImpl.h"

#include <fmt/core.h>
#include <folly/coro/Invoke.h>
#include <folly/coro/Task.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/futures/Future.h>
#include <gtest/gtest.h>

#include <chrono>
#include <memory>
#include <string>
#include <vector>

#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

namespace facebook::eden {
namespace {

using namespace std::chrono_literals;

constexpr folly::StringPiece kRecurseFault{
    "GlobNodeImpl::evaluateImpl::recurse"};
constexpr auto kTimeout = 60s;

/*
 * GlobNodeImpl::evaluateImpl collects the materialized subdirectories it must
 * recurse into while holding the parent's contents lock, then builds a task
 * per entry after releasing it. Those saved names are PathComponentPieces
 * pointing into the parent's PathMap key storage, which any concurrent insert
 * can relocate -- PathMap derives from folly::fbvector and documents that
 * insert and erase invalidate iterators.
 *
 * The fault point sits exactly in that window, so this reproduces the read of
 * a freed name deterministically rather than racing for it.
 *
 * DISABLED_ until the next diff in this stack lands, because against the
 * current code there is no outcome in which this test can pass. It drives
 * the glob into reading a freed name, which is undefined behavior: under
 * @mode/dev-asan the process aborts inside that read, so no assertion is
 * ever reached and the whole binary fails, and under @mode/dev the read
 * returns whatever now occupies the freed bytes, so any assertion over it
 * is nondeterministic. The next diff makes the behavior defined and drops
 * the prefix.
 */
TEST(
    GlobRecurseLifetimeTest,
    DISABLED_deferredRecurseNameSurvivesEntryMapGrowth) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar/file.txt", "contents");
  builder.setFile("other/bar/file.txt", "contents");
  TestMount mount{builder};

  // Materializing a child of "foo" materializes "foo" itself, and only a
  // materialized directory takes the deferred-recursion path: for an
  // unmaterialized one the walk loads the child by object id instead.
  mount.addFile("foo/materialized.txt", "contents");

  auto edenMount = mount.getEdenMount();
  auto serverState = mount.getServerState();
  auto& faultInjector = serverState->getFaultInjector();
  // count=1: only the first check blocks, and the fault removes itself, so
  // the recursion below the parked level runs normally once it is released.
  faultInjector.injectBlock(kRecurseFault, ".*", /*count=*/1);

  folly::CPUThreadPoolExecutor exec{
      2,
      std::make_shared<folly::NamedThreadFactory>("PrefetchFilesV2ThreadPool")};

  PrefetchParams params;
  params.returnPrefetchedFiles() = true;
  auto globFuture =
      folly::coro::co_withExecutor(
          &exec,
          folly::coro::co_invoke(
              [globber = ThriftGlobImpl{params},
               edenMount,
               serverState]() mutable -> folly::coro::Task<folly::Unit> {
                co_await globber.glob(
                    edenMount,
                    serverState,
                    std::vector<std::string>{"*/bar/file.txt"},
                    ObjectFetchContext::getNullContext());
                co_return folly::unit;
              }))
          .start();

  ASSERT_TRUE(faultInjector.waitUntilBlocked(kRecurseFault, kTimeout));

  // The walk is parked holding the name of the materialized directory it is
  // about to recurse into. Grow the root's entry map well past its capacity
  // so it reallocates and frees the storage that name pointed at.
  for (size_t i = 0; i < 512; ++i) {
    mount.addFile(fmt::format("filler{:04}.txt", i), "contents");
  }

  faultInjector.unblock(kRecurseFault, ".*");
  std::move(globFuture).get(kTimeout);
}

} // namespace
} // namespace facebook::eden
