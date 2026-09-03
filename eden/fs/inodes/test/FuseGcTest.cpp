/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef __linux__

#include <chrono>
#include <set>
#include <string>
#include <utility>

#include <folly/CancellationToken.h>
#include <gtest/gtest.h>

#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeFuse.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

namespace {

constexpr auto kTimeout = std::chrono::seconds{10};

/**
 * Exercises the active FUSE invalidation pass of pressure-based GC
 * (TreeInode::invalidateChildrenNotAccessedRecentlyFuse) against a fake FUSE
 * device, observing exactly which FUSE_NOTIFY_INVAL_ENTRY messages it emits.
 */
class FuseGcTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFile("pinned/sub/one.txt", "1\n");
    builder_.setFile("pinned/sub/two.txt", "2\n");
    builder_.setFile("pinned/adjacent.txt", "3\n");
    builder_.setFile("other/child/three.txt", "4\n");
    builder_.setFile("rootfile.txt", "5\n");
    testMount_ = std::make_unique<TestMount>(builder_);
    fuse_ = std::make_shared<FakeFuse>();
    testMount_->startFuseAndWait(fuse_);

    // Load every inode and give each an FS reference, as if the kernel had
    // looked them all up: GC only invalidates entries whose parent and child
    // both have FS references.
    testMount_->getEdenMount()->getRootInode()->incFsRefcount();
    for (const char* dir : {"pinned", "pinned/sub", "other", "other/child"}) {
      testMount_->getTreeInode(RelativePathPiece{dir})->incFsRefcount();
    }
    for (const char* file :
         {"pinned/sub/one.txt",
          "pinned/sub/two.txt",
          "pinned/adjacent.txt",
          "other/child/three.txt",
          "rootfile.txt"}) {
      testMount_->getFileInode(RelativePathPiece{file})->incFsRefcount();
    }
  }

  void TearDown() override {
    fuse_->close();
    testMount_->getEdenMount()
        ->getFsChannelCompletionFuture()
        .within(kTimeout)
        .getVia(testMount_->getServerExecutor().get());
    testMount_.reset();
  }

  uint64_t runGc(
      std::shared_ptr<const folly::F14FastSet<InodeNumber>> pinnedInodes) {
    auto numInvalidated = testMount_->getEdenMount()
                              ->getRootInode()
                              ->handleChildrenNotAccessedRecently(
                                  std::chrono::system_clock::time_point::max(),
                                  ObjectFetchContext::getNullContext(),
                                  /*pressureBased=*/true,
                                  folly::CancellationToken{},
                                  std::move(pinnedInodes))
                              .semi()
                              .via(testMount_->getServerExecutor().get())
                              .within(kTimeout)
                              .getVia(testMount_->getServerExecutor().get());
    // The invalidations were only queued; wait for the invalidation thread
    // to write them to the (fake) FUSE device.
    testMount_->getEdenMount()->flushInvalidations().get(kTimeout);
    return numInvalidated;
  }

  /**
   * Read all FUSE_NOTIFY_INVAL_ENTRY messages the GC pass sent, as
   * (parent inode, entry name) pairs.
   */
  std::set<std::pair<uint64_t, std::string>> readInvalidatedEntries() {
    std::set<std::pair<uint64_t, std::string>> result;
    for (const auto& response : fuse_->getAllResponses()) {
      if (response.header.error != FUSE_NOTIFY_INVAL_ENTRY) {
        continue;
      }
      auto* out = reinterpret_cast<const fuse_notify_inval_entry_out*>(
          response.body.data());
      auto* name = reinterpret_cast<const char*>(response.body.data()) +
          sizeof(fuse_notify_inval_entry_out);
      result.emplace(out->parent, std::string(name, out->namelen));
    }
    return result;
  }

  uint64_t inodeOf(folly::StringPiece path) {
    return testMount_->getInode(RelativePathPiece{path})->getNodeId().get();
  }

  uint64_t rootIno() {
    return testMount_->getEdenMount()->getRootInode()->getNodeId().get();
  }

  FakeTreeBuilder builder_;
  std::unique_ptr<TestMount> testMount_;
  std::shared_ptr<FakeFuse> fuse_;
};

} // namespace

TEST_F(FuseGcTest, pinnedDirectoryChainIsNotInvalidated) {
  // Pin the deep directory "pinned/sub", as if it were some process's cwd.
  auto pins = std::make_shared<folly::F14FastSet<InodeNumber>>();
  pins->insert(testMount_->getTreeInode("pinned/sub"_relpath)->getNodeId());

  auto numInvalidated = runGc(pins);
  auto invalidated = readInvalidatedEntries();

  // Neither the pinned directory's entry nor its ancestor's entry may be
  // invalidated: unhashing either dentry breaks path resolution for the
  // pinning process.
  EXPECT_FALSE(invalidated.contains({inodeOf("pinned"), "sub"}));
  EXPECT_FALSE(invalidated.contains({rootIno(), "pinned"}));

  // Everything else is fair game, including the contents of the pinned
  // directory and siblings of its ancestors.
  const std::set<std::pair<uint64_t, std::string>> expected{
      {inodeOf("pinned/sub"), "one.txt"},
      {inodeOf("pinned/sub"), "two.txt"},
      {inodeOf("pinned"), "adjacent.txt"},
      {rootIno(), "other"},
      {inodeOf("other"), "child"},
      {inodeOf("other/child"), "three.txt"},
      {rootIno(), "rootfile.txt"},
  };
  for (const auto& entry : expected) {
    EXPECT_TRUE(invalidated.contains(entry))
        << "missing invalidation for " << entry.second;
  }
  EXPECT_EQ(expected.size(), numInvalidated);
}

TEST_F(FuseGcTest, withoutPinInformationOnlyFilesAreInvalidated) {
  auto numInvalidated = runGc(nullptr);
  auto invalidated = readInvalidatedEntries();

  // File entries are still invalidated, including inside subdirectories.
  const std::set<std::pair<uint64_t, std::string>> expected{
      {inodeOf("pinned/sub"), "one.txt"},
      {inodeOf("pinned/sub"), "two.txt"},
      {inodeOf("pinned"), "adjacent.txt"},
      {inodeOf("other/child"), "three.txt"},
      {rootIno(), "rootfile.txt"},
  };
  EXPECT_EQ(expected, invalidated);
  EXPECT_EQ(expected.size(), numInvalidated);
}

#endif // __linux__
