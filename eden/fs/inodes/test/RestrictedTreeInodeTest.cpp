/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/TreeInode.h"

#include <folly/coro/GtestHelpers.h>
#include <gtest/gtest.h>
#include <algorithm>
#include <system_error>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

namespace {
template <typename Fn>
void expectEacces(Fn&& fn) {
  try {
    fn();
    FAIL() << "Expected system_error with EACCES";
  } catch (const std::system_error& ex) {
    EXPECT_EQ(ex.code().value(), EACCES);
  }
}

// Helper to construct a restricted TreeInode and register it with the
// InodeMap. Uses TreeInodePtr::makeNew (handles 0→1 refcount transition)
// + InodeMap::inodeCreated (registers for inodePtrFromThis() lookups).
TreeInodePtr makeRestrictedInode(
    TestMount& testMount,
    PathComponentPiece name) {
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto ino = testMount.getEdenMount()->getOverlay()->allocateInodeNumber();
  auto inode = TreeInodePtr::makeNew(
      ino,
      rootInode,
      name,
      S_IFDIR | 0755,
      std::nullopt,
      DirContents{CaseSensitivity::Sensitive},
      std::nullopt,
      /*isRestricted=*/true);
  testMount.getEdenMount()->getInodeMap()->inodeCreated(inode);
  return inode;
}
} // namespace

TEST(RestrictedTreeInode, normalTreeInodeAllowsReaddir) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto context = ObjectFetchContext::getNullContext();
  auto children = rootInode->getChildren(context, /*loadInodes=*/false);

  auto iter =
      std::find_if(children.begin(), children.end(), [](const auto& entry) {
        return entry.first == "dir"_pc;
      });
  ASSERT_NE(iter, children.end());
}

TEST(RestrictedTreeInode, restrictedFlagDeniesAccess) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  auto context = ObjectFetchContext::getNullContext();
  expectEacces(
      [&] { restricted->getOrFindChild("child"_pc, context, false).get(); });
}

TEST(RestrictedTreeInode, statReturnsZeroPermissions) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  auto context = ObjectFetchContext::getNullContext();
  auto st = restricted->stat(context).get();

  EXPECT_TRUE(S_ISDIR(st.st_mode));
  EXPECT_EQ(st.st_mode & 07777, 0);
}

TEST(RestrictedTreeInode, getOrLoadChildReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  auto context = ObjectFetchContext::getNullContext();
  expectEacces(
      [&] { restricted->getOrLoadChild("anything"_pc, context).get(); });
}

TEST(RestrictedTreeInode, mkdirReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  expectEacces([&] {
    restricted->mkdir("newdir"_pc, S_IFDIR | 0755, InvalidationRequired::No);
  });
}

TEST(RestrictedTreeInode, unlinkReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  auto context = ObjectFetchContext::getNullContext();
  expectEacces([&] {
    restricted->unlink("anything"_pc, InvalidationRequired::No, context).get();
  });
}

TEST(RestrictedTreeInode, symlinkReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  expectEacces([&] {
    restricted->symlink("link"_pc, "target", InvalidationRequired::No);
  });
}

TEST(RestrictedTreeInode, mknodReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  expectEacces([&] {
    restricted->mknod("file"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  });
}

TEST(RestrictedTreeInode, setattrReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  auto context = ObjectFetchContext::getNullContext();
  expectEacces([&] { restricted->setattr(DesiredMetadata{}, context).get(); });
}

TEST(RestrictedTreeInode, lockContentsReadThrowsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  expectEacces([&] { restricted->lockContentsRead(); });
}

TEST(RestrictedTreeInode, lockContentsWriteThrowsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);

  expectEacces([&] { restricted->lockContentsWrite(); });
}

TEST(RestrictedTreeInode, unrestricted_treeInodeIsNotRestricted) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto dirInode = testMount.getTreeInode("dir"_relpath);
  EXPECT_FALSE(dirInode->isRestricted());
}

// --- End-to-end tests that go through the real inode loading pipeline ---

class RestrictedTreeInodeEndToEnd : public ::testing::Test {
 protected:
  void SetUp() override {
    FakeTreeBuilder builder;
    builder.setFile("restricted/secret.txt", "secret content");
    builder.setDirIsRestricted("restricted");
    testMount_ = std::make_unique<TestMount>(builder);
  }

  TreeInodePtr getRestrictedInode() {
    return testMount_->getTreeInode("restricted"_relpath);
  }

  std::unique_ptr<TestMount> testMount_;
};

TEST_F(
    RestrictedTreeInodeEndToEnd,
    loadingRestrictedDirCreatesRestrictedTreeInode) {
  auto restrictedInode = getRestrictedInode();
  EXPECT_TRUE(restrictedInode->isRestricted());
}

TEST_F(RestrictedTreeInodeEndToEnd, restrictedDirStatReturnsZeroPermissions) {
  auto restrictedInode = getRestrictedInode();
  auto context = ObjectFetchContext::getNullContext();
  auto st = restrictedInode->stat(context).get();

#ifndef _WIN32
  // Windows stat() doesn't set st_mode for directories (no metadata table).
  EXPECT_TRUE(S_ISDIR(st.st_mode));
  EXPECT_EQ(st.st_mode & 07777, 0);
#endif
}

TEST_F(RestrictedTreeInodeEndToEnd, restrictedDirGetOrFindChildReturnsEACCES) {
  auto restrictedInode = getRestrictedInode();
  auto context = ObjectFetchContext::getNullContext();
  expectEacces([&] {
    restrictedInode->getOrFindChild("secret.txt"_pc, context, false).get();
  });
}

TEST_F(RestrictedTreeInodeEndToEnd, restrictedDirLockContentsReadThrows) {
  auto restrictedInode = getRestrictedInode();
  expectEacces([&] { restrictedInode->lockContentsRead(); });
}

TEST(RestrictedTreeInode, parentListingIncludesRestrictedDir) {
  FakeTreeBuilder builder;
  builder.setFile("parent/normal.txt", "normal content");
  builder.setFile("parent/restricted_child/secret.txt", "secret content");
  builder.setDirIsRestricted("parent/restricted_child");
  TestMount testMount{builder};

  auto parentInode = testMount.getTreeInode("parent"_relpath);
  // Reach into entries to verify the DirEntry-level flag, not the inode.
  auto contents = parentInode->lockContentsRead();

  auto iter = contents->entries.find("restricted_child"_pc);
  ASSERT_NE(iter, contents->entries.end());
  EXPECT_TRUE(iter->second.isDirectory());
  EXPECT_TRUE(iter->second.isRestricted());
}

TEST(
    RestrictedTreeInode,
    fetchRestrictedTreeCachesRestrictionWhenParentMetadataMissing) {
  TestMount testMount;
  auto backingStore = testMount.getBackingStore();

  // Create a restricted child tree that would normally be discovered via
  // parent metadata before the child is loaded.
  auto [secretBlob, secretBlobId] = backingStore->putBlob("secret content");
  secretBlob->setReady();

  auto* restrictedTree = backingStore->putRestrictedTree({
      {"secret.txt", secretBlobId},
  });
  restrictedTree->setReady();
  auto restrictedTreeId = restrictedTree->get().getObjectId();

  // Building the parent from a StoredTree* drops the child's restricted bit
  // from the parent TreeEntry. This simulates the fail-open case where parent
  // metadata is missing even though a direct child fetch still returns a
  // restricted Tree. Cache that live result on the DirEntry so future lookups
  // can short-circuit before fetching again.
  auto* rootTree = backingStore->putTree({
      {"restricted", restrictedTree},
  });
  rootTree->setReady();
  backingStore->putCommit(RootId{"1"}, rootTree)->setReady();
  testMount.initialize(RootId{"1"});

  auto rootInode = testMount.getRootInode();
  {
    // Prove the synthetic setup actually starts with missing parent metadata:
    // the root DirEntry knows the child is a directory, but does not yet have
    // the restricted bit set.
    auto contents = rootInode->lockContentsRead();
    auto it = contents->entries.find("restricted"_pc);
    ASSERT_NE(it, contents->entries.end());
    EXPECT_FALSE(it->second.isRestricted());
  }

  // Nothing has looked up the child yet, so the restricted child tree has not
  // been fetched from the backing store.
  EXPECT_EQ(backingStore->getAccessCount(restrictedTreeId), 0);

  // The first lookup has to fetch the child tree. That fetch returns a
  // restricted Tree, so the resulting inode is restricted and the parent
  // DirEntry cache should be backfilled from the live fetch result.
  auto restrictedInode = testMount.getTreeInode("restricted"_relpath);
  ASSERT_TRUE(restrictedInode->isRestricted());
  EXPECT_EQ(backingStore->getAccessCount(restrictedTreeId), 1);

  {
    // Verify that the first lookup updated the parent-side cache, not just the
    // loaded child inode.
    auto contents = rootInode->lockContentsRead();
    auto it = contents->entries.find("restricted"_pc);
    ASSERT_NE(it, contents->entries.end());
    EXPECT_TRUE(it->second.isRestricted());
  }

  // Drop the loaded child and unload the parent's children so the next lookup
  // has to consult the parent DirEntry metadata again rather than reusing the
  // already-loaded restricted inode.
  restrictedInode.reset();
  rootInode->unloadChildrenNow();

  // The second lookup should now short-circuit from the cached restricted bit
  // on the parent DirEntry, so it still returns a restricted inode without
  // fetching the child tree a second time.
  auto reloadedInode = testMount.getTreeInode("restricted"_relpath);
  EXPECT_TRUE(reloadedInode->isRestricted());
  EXPECT_EQ(backingStore->getAccessCount(restrictedTreeId), 1);
}

TEST(RestrictedTreeInode, nestedRestrictedDirBlocksAccess) {
  FakeTreeBuilder builder;
  builder.setFile("parent/normal.txt", "normal content");
  builder.setFile("parent/restricted_child/secret.txt", "secret content");
  builder.setDirIsRestricted("parent/restricted_child");
  TestMount testMount{builder};

  auto restrictedInode =
      testMount.getTreeInode("parent/restricted_child"_relpath);
  EXPECT_TRUE(restrictedInode->isRestricted());

  auto context = ObjectFetchContext::getNullContext();
  expectEacces([&] {
    restrictedInode->getOrFindChild("secret.txt"_pc, context, false).get();
  });
}

TEST_F(RestrictedTreeInodeEndToEnd, getObjectIdReturnsTrueId) {
  auto restrictedInode = getRestrictedInode();
  EXPECT_TRUE(restrictedInode->getObjectId().has_value());
}

#ifndef _WIN32
TEST_F(RestrictedTreeInodeEndToEnd, getMetadataBypassesAcl) {
  auto restrictedInode = getRestrictedInode();

  auto metadata = restrictedInode->getMetadata();

  EXPECT_TRUE(S_ISDIR(metadata.mode));
}

TEST_F(
    RestrictedTreeInodeEndToEnd,
    getInodeSlowRejectsDotEdenUnderRestrictedDir) {
  auto context = ObjectFetchContext::getNullContext();

  expectEacces([&] {
    auto lookup = testMount_->getEdenMount()->getInodeSlow(
        "restricted/.eden"_relpath, context);
    std::move(lookup).get();
  });
}

TEST(RestrictedTreeInode, getInodeSlowAllowsDotEdenUnderUnrestrictedDir) {
  FakeTreeBuilder builder;
  builder.setFile("visible/file.txt", "content");
  TestMount testMount{builder};

  auto context = ObjectFetchContext::getNullContext();
  auto throughVisible =
      testMount.getEdenMount()->getInodeSlow("visible/.eden"_relpath, context);
  auto dotEdenThisDir =
      testMount.getEdenMount()->getInodeSlow(".eden/this-dir"_relpath, context);
  auto throughVisibleInode = std::move(throughVisible).get();
  auto dotEdenThisDirInode = std::move(dotEdenThisDir).get();

  EXPECT_EQ(throughVisibleInode->getNodeId(), dotEdenThisDirInode->getNodeId());
}
#endif

TEST(RestrictedTreeInode, renameFromRestrictedDirReturnsEACCES) {
  // Renaming FROM a restricted directory should fail because the source
  // parent's checkAccess() fires before any lock acquisition.
  FakeTreeBuilder builder;
  builder.setFile("restricted/file.txt", "content");
  builder.setDirIsRestricted("restricted");
  builder.setFile("dest/other.txt", "other");
  TestMount testMount{builder};

  auto restricted = testMount.getTreeInode("restricted"_relpath);
  auto dest = testMount.getTreeInode("dest"_relpath);

  expectEacces([&] {
    restricted
        ->rename(
            "file.txt"_pc,
            dest,
            "moved.txt"_pc,
            InvalidationRequired::No,
            ObjectFetchContext::getNullContext())
        .get();
  });
}

TEST(RestrictedTreeInode, renameIntoRestrictedDirReturnsEACCES) {
  // Renaming INTO a restricted directory should fail because the destination
  // parent is checked via materialize() and TreeRenameLocks::acquireLocks(),
  // both of which call lockContentsWrite() -> checkAccess().
  FakeTreeBuilder builder;
  builder.setFile("src/file.txt", "content");
  builder.setFile("restricted/existing.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto src = testMount.getTreeInode("src"_relpath);
  auto restricted = testMount.getTreeInode("restricted"_relpath);

  expectEacces([&] {
    src->rename(
           "file.txt"_pc,
           restricted,
           "moved.txt"_pc,
           InvalidationRequired::No,
           ObjectFetchContext::getNullContext())
        .get();
  });
}

CO_TEST(RestrictedTreeInode, co_getChildrenOnRestrictedTreeReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("restricted"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(vi.co_getChildren(
      "restricted"_relpath, edenMount->getObjectStore(), context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), EACCES);
  }
}

CO_TEST(
    RestrictedTreeInode,
    co_getChildrenSkipsBackingStoreFetchForRestrictedChild) {
  // Parent is unrestricted; one child entry is restricted. The coro path
  // must hand back a synthesized restricted VirtualInode for that child
  // instead of fetching its tree from the backing store.
  FakeTreeBuilder builder;
  builder.setFile("parent/normal/file.txt", "ok");
  builder.setFile("parent/restricted_child/secret.txt", "secret");
  builder.setDirIsRestricted("parent/restricted_child");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await vi.co_getChildren(
      "parent"_relpath, edenMount->getObjectStore(), context);
  bool sawRestrictedChild = false;
  bool sawNormalChild = false;
  for (auto& [name, tryVi] : results) {
    if (name == "restricted_child"_pc) {
      sawRestrictedChild = true;
      CO_ASSERT_TRUE(tryVi.hasValue());
      // Reading children of the synthesized restricted VirtualInode must
      // surface EACCES — it is a real restricted view, not the underlying
      // tree contents.
      auto childResult =
          co_await folly::coro::co_awaitTry(tryVi.value().co_getChildren(
              "parent/restricted_child"_relpath,
              edenMount->getObjectStore(),
              context));
      EXPECT_TRUE(childResult.hasException<std::system_error>());
      if (auto* ex =
              childResult.exception().get_exception<std::system_error>()) {
        EXPECT_EQ(ex->code().value(), EACCES);
      }
    } else if (name == "normal"_pc) {
      sawNormalChild = true;
      CO_ASSERT_TRUE(tryVi.hasValue());
    }
  }
  EXPECT_TRUE(sawRestrictedChild);
  EXPECT_TRUE(sawNormalChild);
}

// Exercises the InodePtr branch — the dominant production path after a
// mount has loaded inodes.
CO_TEST(RestrictedTreeInode, co_getChildren_inodePtrBranchReturnsEntries) {
  FakeTreeBuilder builder;
  builder.setFile("dir/a.txt", "a");
  builder.setFile("dir/b.txt", "b");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto dirInode = testMount.getTreeInode("dir"_relpath);
  TestMount::loadAllInodes(dirInode);
  VirtualInode vi{InodePtr{dirInode}};
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await vi.co_getChildren(
      "dir"_relpath, edenMount->getObjectStore(), context);
  EXPECT_EQ(results.size(), 2);
  for (auto& [name, tryVi] : results) {
    CO_ASSERT_TRUE(tryVi.hasValue());
  }
}

// Catches a regression where the early isDirectory() guard drifts out of
// sync with the variant arms.
CO_TEST(RestrictedTreeInode, co_getChildren_onFileReturnsENOTDIR) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("dir/file.txt"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(vi.co_getChildren(
      "dir/file.txt"_relpath, edenMount->getObjectStore(), context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), ENOTDIR);
  }
}

// ============================================================================
// Coroutine readdir ACL parity (regression for TreeInode coro recheck gate).
// Mirrors the futures-side gate in TreeInode::getChildren
// (recheckPermissionIfExpired before lock) so the coroutine variant does not
// stick on stale EACCES after a TTL-expired permission grant.
// ============================================================================

CO_TEST(RestrictedTreeInode, co_getChildren_returnsEntriesOnUnrestrictedRoot) {
  // Smoke test for the success path: recheck short-circuits on a
  // non-restricted directory and entries are returned without EACCES.
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await rootInode->co_getChildren(context);
  bool sawDir = false;
  for (auto& [name, tryVi] : results) {
    if (name == "dir"_pc) {
      sawDir = true;
      CO_ASSERT_TRUE(tryVi.hasValue());
    }
  }
  EXPECT_TRUE(sawDir);
}

// Exercises the wlock + loadChild + inodeLoadCleanUps + SCOPE_EXIT
// discipline in TreeInode::co_getChildren that the loadInodes=false default
// never reaches (rlockCheckChild returns an inline VirtualInode for
// unmaterialized entries). loadInodes=true forces every non-loaded entry
// through loadChild, which queues a LoadChildCleanUp drained by SCOPE_EXIT
// after the lock releases.
CO_TEST(RestrictedTreeInode, co_getChildren_loadInodesTrueExercisesLoadChild) {
  FakeTreeBuilder builder;
  builder.setFile("dir/a.txt", "a");
  builder.setFile("dir/b.txt", "b");
  builder.setFile("dir/c.txt", "c");
  TestMount testMount{builder};

  auto dirInode = testMount.getTreeInode("dir"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto results =
      co_await dirInode->co_getChildren(context, /*loadInodes=*/true);
  EXPECT_EQ(results.size(), 3);
  for (auto& [name, tryVi] : results) {
    CO_ASSERT_TRUE(tryVi.hasValue());
    EXPECT_TRUE(tryVi.value().asInodePtr() != nullptr);
  }
}

// Exercises the EACCES-on-lockContentsWrite path. The SCOPE_EXIT is already
// registered when lockContentsWrite() throws — this verifies the empty
// inodeLoadCleanUps unwind branch is benign.
CO_TEST(RestrictedTreeInode, co_getChildren_restrictedThrowsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(
      restricted->co_getChildren(context, /*loadInodes=*/false));
  CO_ASSERT_TRUE(result.hasException());
  auto* err = result.tryGetExceptionObject<std::system_error>();
  CO_ASSERT_NE(err, nullptr);
  EXPECT_EQ(err->code().value(), EACCES);
}
