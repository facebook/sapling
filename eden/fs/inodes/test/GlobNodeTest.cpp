/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/GlobNode.h"
#include "eden/fs/utils/GlobResult.h"

#include <utility>

#include <folly/Conv.h>
#include <folly/Exception.h>
#include <folly/Range.h>
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

namespace {
constexpr folly::Duration kSmallTimeout =
    std::chrono::duration_cast<folly::Duration>(1s);

/**
 * Issue a glob request.
 *
 * Note: This future executes on the server executor which thus needs to be
 * manually drained for the returned Future to be ready.
 */
folly::Future<std::vector<GlobResult>> evaluateGlob(
    TestMount& mount,
    GlobNode& globRoot,
    std::shared_ptr<PrefetchList> prefetchIds,
    const RootId& commitId) {
  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto globResults =
      std::make_shared<folly::Synchronized<std::vector<GlobResult>>>();
  return globRoot
      .evaluate(
          std::move(objectStore),
          ObjectFetchContext::getNullContext(),
          RelativePathPiece(),
          rootInode,
          prefetchIds.get(),
          *globResults,
          commitId)
      .thenValue([globResults](auto&&) {
        std::vector<GlobResult> result;
        std::swap(result, *globResults->wlock());
        return result;
      })
      .semi()
      .via(mount.getServerExecutor().get());
}

const RootId kZeroRootId{};

} // namespace

enum StartReady : bool {
  Defer = false,
  Start = true,
};

enum Prefetch : bool {
  NoPrefetch = false,
  PrefetchBlobs = true,
};

class GlobNodeTest : public ::testing::TestWithParam<
                         std::pair<enum StartReady, enum Prefetch>> {
 protected:
  void SetUp() override {
    // The file contents are coupled with AId, BId and WatId below.
    builder_.setFiles(
        {{"dir/a.txt", "a"},
         {"dir/sub/b.txt", "b"},
         {".watchmanconfig", "wat"}});
    mount_.initialize(builder_, /*startReady=*/GetParam().first);
    prefetchIds_ = nullptr;
  }

  std::vector<GlobResult> doGlob(
      folly::StringPiece pattern,
      bool includeDotfiles,
      const RootId& commitId) {
    GlobNode globRoot(
        /*includeDotfiles=*/includeDotfiles,
        mount_.getConfig()->getCaseSensitive());
    globRoot.parse(pattern);
    return doGlob(globRoot, commitId);
  }

  std::vector<GlobResult> doGlob(GlobNode& globRoot, const RootId& commitId) {
    globRoot.debugDump();

    if (shouldPrefetch()) {
      prefetchIds_ = std::make_shared<GlobNode::PrefetchList>();
    }

    auto future = evaluateGlob(mount_, globRoot, prefetchIds_, commitId);

    if (!GetParam().first) {
      builder_.setAllReady();
    }
    mount_.drainServerExecutor();
    return std::move(future).get(kSmallTimeout);
  }

  std::vector<GlobResult> doGlobIncludeDotFiles(
      folly::StringPiece pattern,
      const RootId& commitId) {
    return doGlob(pattern, true, commitId);
  }

  std::vector<GlobResult> doGlobExcludeDotFiles(
      folly::StringPiece pattern,
      const RootId& commitId) {
    return doGlob(pattern, false, commitId);
  }

  bool shouldPrefetch() const {
    return GetParam().second;
  }

  std::vector<ObjectId> getPrefetchIds() const {
    return *prefetchIds_->rlock();
  }

  TestMount mount_;
  FakeTreeBuilder builder_;
  std::shared_ptr<GlobNode::PrefetchList> prefetchIds_;
};

TEST_P(GlobNodeTest, starTxt) {
  auto matches = doGlobIncludeDotFiles("*.txt", kZeroRootId);
  EXPECT_TRUE(matches.empty());
  if (shouldPrefetch()) {
    EXPECT_TRUE(getPrefetchIds().empty());
  }
}

// id of "a"
const ObjectId AId =
    ObjectId::fromHex("86f7e437faa5a7fce15d1ddcb9eaeaea377667b8");
// id of "b"
const ObjectId BId =
    ObjectId::fromHex("e9d71f5ee7c92d6dc9e92ffdad17b8bd49418f98");
// id of "wat"
const ObjectId WatId =
    ObjectId::fromHex("a3bbe1a8f2f025b8b6c5b66937763bb2b9bebdf2");

TEST_P(GlobNodeTest, matchFilesByExtensionRecursively) {
  auto matches = doGlobIncludeDotFiles("**/*.txt", kZeroRootId);

  std::vector<GlobResult> expect{
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);

  if (shouldPrefetch()) {
    std::vector<ObjectId> expectIds{AId, BId};
    EXPECT_EQ(expectIds, getPrefetchIds());
  }
}

TEST_P(GlobNodeTest, star) {
  auto matches = doGlobIncludeDotFiles("*", kZeroRootId);

  std::vector<GlobResult> expect{
      GlobResult(".eden"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult(".watchmanconfig"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir"_relpath, dtype_t::Dir, kZeroRootId)};
  EXPECT_EQ(expect, matches);

  if (shouldPrefetch()) {
    std::vector<ObjectId> expectIds{WatId};
    EXPECT_EQ(expectIds, getPrefetchIds());
  }
}

TEST_P(GlobNodeTest, starExcludeDot) {
  auto matches = doGlobExcludeDotFiles("*", kZeroRootId);

  std::vector<GlobResult> expect{
      GlobResult("dir"_relpath, dtype_t::Dir, kZeroRootId)};
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, starStarExcludeDot) {
  auto matches = doGlobExcludeDotFiles("dir/sub/**/sub/b.txt", kZeroRootId);

  std::vector<GlobResult> expect;
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, starStarRootExcludeDot) {
  auto matches = doGlobExcludeDotFiles("**/root", kZeroRootId);

  std::vector<GlobResult> expect;
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, starStarBeginning) {
  auto matches = doGlobExcludeDotFiles("**/sub/b.txt", kZeroRootId);

  std::vector<GlobResult> expect{
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}

#ifndef _WIN32
TEST_P(GlobNodeTest, recursiveTxtWithChanges) {
  // Ensure that we enumerate things from the overlay
  mount_.addFile("root.txt", "added\n");
  mount_.addSymlink("sym.txt", "root.txt");
  // The mode change doesn't directly impact the results, but
  // does cause us to materialize this entry.  We just want
  // to make sure that it continues to show up after the change.
  builder_.setReady("dir");
  builder_.setReady("dir/a.txt");
  mount_.chmod("dir/a.txt", 0777);

  auto matches = doGlobIncludeDotFiles("**/*.txt", kZeroRootId);

  std::vector<GlobResult> expect{
      GlobResult("root.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("sym.txt"_relpath, dtype_t::Symlink, kZeroRootId),
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);

  if (shouldPrefetch()) {
    std::vector<ObjectId> expectIds{
        // No root.txt, as it is in the overlay
        // No sym.txt, as it is in the overlay
        // No AId as we chmod'd the file and thus materialized it
        BId};
    EXPECT_EQ(expectIds, getPrefetchIds());
  }
}
#endif

TEST_P(GlobNodeTest, matchGlobDirectoryAndDirectoryChild) {
  GlobNode globRoot(
      /*includeDotfiles=*/false, mount_.getConfig()->getCaseSensitive());
  globRoot.parse("dir/*");
  globRoot.parse("dir/*/*");

  auto matches = doGlob(globRoot, kZeroRootId);
  std::vector<GlobResult> expect{
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir/sub"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, matchGlobDirectoryAndDirectoryRecursiveChildren) {
  GlobNode globRoot(
      /*includeDotfiles=*/false, mount_.getConfig()->getCaseSensitive());
  globRoot.parse("dir/*");
  globRoot.parse("dir/*/**");

  auto matches = doGlob(globRoot, kZeroRootId);
  std::vector<GlobResult> expect{
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir/sub"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, matchLiteralDirectoryAndDirectoryChild) {
  GlobNode globRoot(
      /*includeDotfiles=*/false, mount_.getConfig()->getCaseSensitive());
  globRoot.parse("dir");
  globRoot.parse("dir/a.txt");

  auto matches = doGlob(globRoot, kZeroRootId);
  std::vector<GlobResult> expect{
      GlobResult("dir"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}

TEST_P(GlobNodeTest, matchLiteralDirectoryAndDirectoryRecursiveChildren) {
  GlobNode globRoot(
      /*includeDotfiles=*/false, mount_.getConfig()->getCaseSensitive());
  globRoot.parse("dir");
  globRoot.parse("dir/**");

  auto matches = doGlob(globRoot, kZeroRootId);
  std::vector<GlobResult> expect{
      GlobResult("dir"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("dir/sub"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}

const std::pair<enum StartReady, enum Prefetch> combinations[] = {
    {StartReady::Start, Prefetch::NoPrefetch},
    {StartReady::Start, Prefetch::PrefetchBlobs},
    {StartReady::Defer, Prefetch::NoPrefetch},
    {StartReady::Defer, Prefetch::PrefetchBlobs},
};

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
INSTANTIATE_TEST_CASE_P(Glob, GlobNodeTest, ::testing::ValuesIn(combinations));
#pragma clang diagnostic pop

TEST(GlobNodeTest, matchingDirectoryDoesNotLoadTree) {
  auto mount = TestMount{};
  auto builder = FakeTreeBuilder{};
  builder.setFiles({{"dir/subdir/file", ""}});
  mount.initialize(builder, /*startReady=*/false);
  builder.setReady("dir");
  ASSERT_FALSE(
      mount.getEdenMount()
          ->getInodeSlow(
              "dir/subdir"_relpath, ObjectFetchContext::getNullContext())
          .semi()
          .isReady())
      << "Loading dir/subdir should hang indefinitely";

  for (folly::StringPiece pattern : {"dir/*"_sp, "dir/subdir"_sp}) {
    SCOPED_TRACE(folly::to<std::string>("pattern = ", pattern));
    GlobNode globRoot(
        /*includeDotfiles=*/false, mount.getConfig()->getCaseSensitive());
    globRoot.parse("dir/*");
    globRoot.debugDump();

    auto matches = std::vector<GlobResult>{};
    try {
      auto fut =
          evaluateGlob(mount, globRoot, /*prefetchIds=*/nullptr, kZeroRootId);
      mount.drainServerExecutor();
      matches = std::move(fut).get(kSmallTimeout);
    } catch (const folly::FutureTimeout&) {
      FAIL() << "Matching dir/subdir should not load dir/subdir";
    }

    EXPECT_FALSE(
        mount.getEdenMount()
            ->getInodeSlow(
                "dir/subdir"_relpath, ObjectFetchContext::getNullContext())
            .semi()
            .isReady())
        << "dir/subdir should still be unloaded after evaluating glob";
    EXPECT_EQ(
        (std::vector<GlobResult>{
            GlobResult("dir/subdir"_relpath, dtype_t::Dir, kZeroRootId),
        }),
        matches);
  }
}

TEST(GlobNodeTest, treeLoadError) {
  auto mount = TestMount{};
  auto builder = FakeTreeBuilder{};
  builder.setFiles({
      {"dir/a/foo.txt", "foo"},
      {"dir/a/b/a.txt", "foo"},
      {"dir/a/b/b.txt", "foo"},
      {"dir/b/a/a.txt", "foo"},
      {"dir/b/a/b.txt", "foo"},
      {"dir/c/a/a.txt", "foo"},
      {"dir/c/x.txt", "foo"},
      {"dir/c/y.txt", "foo"},
  });
  mount.initialize(builder, /*startReady=*/false);
  builder.setReady("dir");
  builder.setReady("dir/a");

  {
    GlobNode globRoot(
        /*includeDotfiles=*/false, mount.getConfig()->getCaseSensitive());
    globRoot.parse("dir/**/a.txt");

    auto globFuture =
        evaluateGlob(mount, globRoot, /*prefetchIds=*/nullptr, kZeroRootId);
    mount.drainServerExecutor();
    EXPECT_FALSE(globFuture.isReady())
        << "glob should not finish when some subtrees are not read";

    // Cause dir/a/b to fail to load
    builder.triggerError("dir/a/b", std::runtime_error("cosmic radiation"));
    mount.drainServerExecutor();

    // We still haven't allowed the rest of the trees to finish loading,
    // so the glob shouldn't be finished yet.
    //
    // This test case is checking for a regression where the globFuture would
    // complete early when an error occurred processing one TreeInode, even
    // though work was still being done to process the glob on other subtrees.
    // Completion of the globFuture signals the caller that they can destroy the
    // GlobNode, but this isn't safe if there is still work in progress to
    // evaluate it, even if that work will eventually get discarded due to the
    // original error.
    EXPECT_FALSE(globFuture.isReady())
        << "glob should not finish early when still waiting on some trees";

    // Mark all of the remaining trees ready, which should allow the glob
    // evaluation to complete.
    builder.setAllReady();
    mount.drainServerExecutor();
    try {
      auto result = std::move(globFuture).get(kSmallTimeout);
      FAIL() << "glob should have succeeded";
    } catch (const std::runtime_error& ex) {
      EXPECT_THAT(ex.what(), testing::HasSubstr("cosmic radiation"));
    } catch (const folly::FutureTimeout&) {
      FAIL() << "glob did not finish";
    }
  }
}

TEST_P(GlobNodeTest, testCommitIdSet) {
  const RootId randomId{"37ce5515c1b313ce722366c31c10db0883fff7e0"};

  auto matches = doGlobIncludeDotFiles("**/*.txt", randomId);

  std::vector<GlobResult> expect{
      GlobResult("dir/a.txt"_relpath, dtype_t::Regular, randomId),
      GlobResult("dir/sub/b.txt"_relpath, dtype_t::Regular, randomId),
  };
  EXPECT_EQ(expect, matches);

  if (shouldPrefetch()) {
    std::vector<ObjectId> expectIds{AId, BId};
    EXPECT_EQ(expectIds, getPrefetchIds());
  }
}

TEST(GlobNodeTest, testCaseInsensitive) {
  auto mount = TestMount{CaseSensitivity::Insensitive};
  auto builder = FakeTreeBuilder{};
  builder.setFiles({{"case/MIXEDcase", "a"}, {"Foo/Bar", ""}, {"Foo/Baz", ""}});
  mount.initialize(builder, /*startReady=*/true);

  GlobNode globRoot(
      /*includeDotfiles=*/false, mount.getConfig()->getCaseSensitive());
  globRoot.parse("Case");
  globRoot.parse("CASE/MixedCase");
  globRoot.parse("CASE/MixedCase");
  globRoot.parse("f*/b?z");

  auto matches = std::vector<GlobResult>{};
  auto fut =
      evaluateGlob(mount, globRoot, /*prefetchIds=*/nullptr, kZeroRootId);
  mount.drainServerExecutor();
  matches = std::move(fut).get(kSmallTimeout);

  std::vector<GlobResult> expect{
      GlobResult("case"_relpath, dtype_t::Dir, kZeroRootId),
      GlobResult("case/MIXEDcase"_relpath, dtype_t::Regular, kZeroRootId),
      GlobResult("Foo/Baz"_relpath, dtype_t::Regular, kZeroRootId),
  };
  EXPECT_EQ(expect, matches);
}
