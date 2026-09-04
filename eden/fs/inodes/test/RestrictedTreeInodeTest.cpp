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
#include <memory>
#include <system_error>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/RestrictedContentMode.h"
#include "eden/fs/fuse/FuseDirList.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeMetadata.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
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

TreeInodePtr makeTreeInodeChildWithoutParentEntry(
    TestMount& testMount,
    TreeInodePtr parent,
    PathComponentPiece name) {
  auto ino = testMount.getEdenMount()->getOverlay()->allocateInodeNumber();
  auto inode = TreeInodePtr::makeNew(
      ino,
      parent,
      name,
      S_IFDIR | 0755,
      std::nullopt,
      DirContents{CaseSensitivity::Sensitive},
      std::nullopt);
  testMount.getEdenMount()->getInodeMap()->inodeCreated(inode);
  return inode;
}

// Shared setup for the omittedMode_* tests: a parent directory with one
// normal child and one restricted child, mounted with
// acl:restricted-content-mode = omitted. The mode is snapshotted by the
// mount's ObjectStore at construction, so it must be set on the EdenConfig
// before construction rather than via updateEdenConfig().
std::unique_ptr<TestMount> makeOmittedModeTestMount() {
  FakeTreeBuilder builder;
  builder.setFile("parent/normal.txt", "normal content");
  builder.setFile("parent/restricted_child/secret.txt", "secret content");
  builder.setDirIsRestricted("parent/restricted_child");
  return std::make_unique<TestMount>(
      builder,
      /*startReady=*/true,
      /*enableActivityBuffer=*/true,
      kPathMapDefaultCaseSensitive,
      /*errorLogger=*/nullptr,
      [](EdenConfig& config) {
        config.restrictedContentMode.setValue(
            RestrictedContentMode::Omitted, ConfigSourceType::CommandLine);
      });
}
} // namespace

CO_TEST(RestrictedTreeInode, normalTreeInodeAllowsReaddir) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto context = ObjectFetchContext::getNullContext();
  auto children =
      co_await rootInode->getChildren(context, /*loadInodes=*/false);

  auto iter =
      std::find_if(children.begin(), children.end(), [](const auto& entry) {
        return entry.first == "dir"_pc;
      });
  CO_ASSERT_NE(iter, children.end());
  CO_ASSERT_TRUE(iter->second.hasValue());
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

#ifndef _WIN32
TEST(RestrictedTreeInode, lastChildReferenceUnderRestrictedParentBypassesAcl) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);
  auto child =
      makeTreeInodeChildWithoutParentEntry(testMount, restricted, "child"_pc);
  auto childIno = child->getNodeId();
  child->incFsRefcount();

  EXPECT_NO_THROW(child.reset());
  EXPECT_NO_THROW(
      testMount.getEdenMount()->getInodeMap()->decFsRefcount(childIno, 1));

  // This synthetic child is still linked, so the InodeMap keeps it loaded via
  // onLinkedInodeUnreferenced(). The regression covered here is that dropping
  // the last references does not try to take the restricted parent's public
  // contents lock and throw EACCES.
  EXPECT_TRUE(
      testMount.getEdenMount()->getInodeMap()->isInodeLoadedOrRemembered(
          childIno));
}

TEST(
    RestrictedTreeInode,
    lastChildReferenceUnderRestrictedParentWithLoadedEntryBypassesAcl) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);
  auto child =
      makeTreeInodeChildWithoutParentEntry(testMount, restricted, "child"_pc);
  auto childIno = child->getNodeId();

  {
    auto contents = restricted->getContentsUnchecked().wlock();
    auto [entry, inserted] =
        contents->entries.emplace("child"_pc, S_IFDIR | 0755, childIno);
    ASSERT_TRUE(inserted);
    entry->second.setInode(child.get());
  }

  child->incFsRefcount();
  EXPECT_NO_THROW(child.reset());
  EXPECT_NO_THROW(
      testMount.getEdenMount()->getInodeMap()->decFsRefcount(childIno, 1));

  auto contents = restricted->getContentsUnchecked().rlock();
  auto entry = contents->entries.find("child"_pc);
  ASSERT_NE(entry, contents->entries.end());
  EXPECT_NE(nullptr, entry->second.getInode());
  EXPECT_EQ(childIno, entry->second.getInodeNumber());
}
#endif

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
    auto restrictedObjectId =
        builder.getRoot()->get().find("restricted"_pc)->second.getObjectId();
    testMount_->getBackingStore()->setCheckPermissionResult(
        restrictedObjectId, false);
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

  // Build the parent entry directly so its metadata is missing even though a
  // direct child fetch still returns a restricted Tree. Cache that live result
  // on the DirEntry so future lookups can short-circuit before fetching again.
  Tree::container rootEntries{kPathMapDefaultCaseSensitive};
  rootEntries.emplace(
      "restricted"_pc,
      ObjectId{restrictedTreeId},
      TreeEntryType::TREE,
      /* isRestricted */ false,
      /* hasACL */ std::nullopt);
  auto* rootTree = backingStore->putTree(std::move(rootEntries));
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
  testMount.getBackingStore()->setCheckPermissionResult(
      restrictedInode->getObjectId().value(), false);

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

CO_TEST(RestrictedTreeInode, getChildrenOnRestrictedTreeReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("restricted"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(vi.getChildren(
      "restricted"_relpath, edenMount->getObjectStore(), context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), EACCES);
  }
}

CO_TEST(
    RestrictedTreeInode,
    getChildrenSkipsBackingStoreFetchForRestrictedChild) {
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

  auto results = co_await vi.getChildren(
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
          co_await folly::coro::co_awaitTry(tryVi.value().getChildren(
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
CO_TEST(RestrictedTreeInode, getChildren_inodePtrBranchReturnsEntries) {
  FakeTreeBuilder builder;
  builder.setFile("dir/a.txt", "a");
  builder.setFile("dir/b.txt", "b");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto dirInode = testMount.getTreeInode("dir"_relpath);
  TestMount::loadAllInodes(dirInode);
  VirtualInode vi{InodePtr{dirInode}};
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await vi.getChildren(
      "dir"_relpath, edenMount->getObjectStore(), context);
  EXPECT_EQ(results.size(), 2);
  for (auto& [name, tryVi] : results) {
    CO_ASSERT_TRUE(tryVi.hasValue());
  }
}

// Catches a regression where the early isDirectory() guard drifts out of
// sync with the variant arms.
CO_TEST(RestrictedTreeInode, getChildren_onFileReturnsENOTDIR) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("dir/file.txt"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(vi.getChildren(
      "dir/file.txt"_relpath, edenMount->getObjectStore(), context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), ENOTDIR);
  }
}

CO_TEST(
    RestrictedTreeInode,
    co_getChildrenAttributes_onRestrictedTreeReturnsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("restricted"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto attrs = ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE | ENTRY_ATTRIBUTE_SIZE;

  auto result = co_await folly::coro::co_awaitTry(vi.co_getChildrenAttributes(
      attrs,
      RelativePath{"restricted"},
      edenMount->getObjectStore(),
      edenMount->getLastCheckoutTime().toTimespec(),
      context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), EACCES);
  }
}

// Dominant production path after a mount has loaded inodes.
CO_TEST(
    RestrictedTreeInode,
    co_getChildrenAttributes_inodePtrBranchReturnsAttrs) {
  FakeTreeBuilder builder;
  builder.setFile("dir/a.txt", "a");
  builder.setFile("dir/b.txt", "b");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto dirInode = testMount.getTreeInode("dir"_relpath);
  TestMount::loadAllInodes(dirInode);
  VirtualInode vi{InodePtr{dirInode}};
  auto context = ObjectFetchContext::getNullContext();
  auto attrs = ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE | ENTRY_ATTRIBUTE_SIZE;

  auto results = co_await vi.co_getChildrenAttributes(
      attrs,
      RelativePath{"dir"},
      edenMount->getObjectStore(),
      edenMount->getLastCheckoutTime().toTimespec(),
      context);
  EXPECT_EQ(results.size(), 2);
  for (auto& [name, tryAttrs] : results) {
    CO_ASSERT_TRUE(tryAttrs.hasValue());
  }
}

CO_TEST(RestrictedTreeInode, co_getChildrenAttributes_onFileReturnsENOTDIR) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("dir/file.txt"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto attrs = ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE | ENTRY_ATTRIBUTE_SIZE;

  auto result = co_await folly::coro::co_awaitTry(vi.co_getChildrenAttributes(
      attrs,
      RelativePath{"dir/file.txt"},
      edenMount->getObjectStore(),
      edenMount->getLastCheckoutTime().toTimespec(),
      context));
  CO_ASSERT_TRUE(result.hasException());
  EXPECT_TRUE(result.hasException<std::system_error>());
  if (auto* ex = result.exception().get_exception<std::system_error>()) {
    EXPECT_EQ(ex->code().value(), ENOTDIR);
  }
}

// Covers the file-inline, restricted-child, and non-restricted-tree per-child
// dispatch arms in one shot.
CO_TEST(
    RestrictedTreeInode,
    co_getChildrenAttributes_treePtrBranchReturnsMixedAttrs) {
  FakeTreeBuilder builder;
  builder.setFile("parent/normal/file.txt", "ok");
  builder.setFile("parent/leaf.txt", "leaf");
  builder.setFile("parent/restricted_child/secret.txt", "secret");
  builder.setDirIsRestricted("parent/restricted_child");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto vi = testMount.getVirtualInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto attrs = ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE;

  auto results = co_await vi.co_getChildrenAttributes(
      attrs,
      RelativePath{"parent"},
      edenMount->getObjectStore(),
      edenMount->getLastCheckoutTime().toTimespec(),
      context);
  bool sawNormal = false;
  bool sawLeaf = false;
  bool sawRestricted = false;
  for (auto& [name, tryAttrs] : results) {
    if (name == "normal"_pc) {
      sawNormal = true;
      CO_ASSERT_TRUE(tryAttrs.hasValue());
    } else if (name == "leaf.txt"_pc) {
      sawLeaf = true;
      CO_ASSERT_TRUE(tryAttrs.hasValue());
    } else if (name == "restricted_child"_pc) {
      sawRestricted = true;
      CO_ASSERT_TRUE(tryAttrs.hasValue());
    }
  }
  EXPECT_TRUE(sawNormal);
  EXPECT_TRUE(sawLeaf);
  EXPECT_TRUE(sawRestricted);
}

TEST(RestrictedTreeInode, isRestricted_reflectsVariant) {
  FakeTreeBuilder builder;
  builder.setFile("normal/file.txt", "ok");
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  // Unloaded directories (TreePtr variant).
  EXPECT_TRUE(testMount.getVirtualInode("restricted"_relpath).isRestricted());
  EXPECT_FALSE(testMount.getVirtualInode("normal"_relpath).isRestricted());

  // Loaded directories (InodePtr variant).
  auto restrictedInode = makeRestrictedInode(testMount, "loaded_restricted"_pc);
  EXPECT_TRUE(VirtualInode{InodePtr{restrictedInode}}.isRestricted());
  auto normalInode = testMount.getTreeInode("normal"_relpath);
  EXPECT_FALSE(VirtualInode{InodePtr{normalInode}}.isRestricted());
}

CO_TEST(
    RestrictedTreeInode,
    co_getDigestHash_restrictedTreePtrWithholdsDigestHash) {
  FakeTreeBuilder builder;
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto restricted = testMount.getVirtualInode("restricted"_relpath);
  CO_ASSERT_EQ(
      VirtualInode::ContainedType::Tree, restricted.testGetContainedType());
  CO_ASSERT_TRUE(restricted.isRestricted());

  auto result = co_await folly::coro::co_awaitTry(restricted.co_getDigestHash(
      "restricted"_relpath,
      testMount.getEdenMount()->getObjectStore(),
      ObjectFetchContext::getNullContext()));
  CO_ASSERT_TRUE(result.hasValue());
  EXPECT_FALSE(result.value().has_value());
}

TEST(RestrictedTreeInode, tryGetEntryAttributesSync_restrictedDirWithholdsAux) {
  FakeTreeBuilder builder;
  builder.setFile("normal/file.txt", "ok");
  builder.setFile("restricted/secret.txt", "secret");
  builder.setDirIsRestricted("restricted");
  TestMount testMount{builder};

  auto edenMount = testMount.getEdenMount();
  auto objectStore = edenMount->getObjectStore();
  auto context = ObjectFetchContext::getNullContext();
  auto checkoutTime = edenMount->getLastCheckoutTime().toTimespec();
  auto attrs = ENTRY_ATTRIBUTE_DIGEST_HASH | ENTRY_ATTRIBUTE_DIGEST_SIZE;

  auto restrictedVi = testMount.getVirtualInode("restricted"_relpath);
  EXPECT_TRUE(restrictedVi.isRestricted());
  auto restrictedResult = restrictedVi.tryGetEntryAttributesSync(
      attrs, "restricted"_relpath, objectStore, checkoutTime, context);
  EXPECT_TRUE(restrictedResult.has_value());
  if (restrictedResult.has_value()) {
    EXPECT_FALSE(restrictedResult->digestHash.has_value());
    EXPECT_FALSE(restrictedResult->digestSize.has_value());
  }

  auto normalVi = testMount.getVirtualInode("normal"_relpath);
  EXPECT_FALSE(normalVi.isRestricted());
  auto normalResult = normalVi.tryGetEntryAttributesSync(
      attrs, "normal"_relpath, objectStore, checkoutTime, context);
  if (normalResult.has_value()) {
    EXPECT_TRUE(normalResult->digestHash.has_value());
  }
}

// ============================================================================
// Coroutine getChildren ACL parity (regression for TreeInode recheck gate).
// Ensures recheckPermissionIfExpired runs before the contents lock so
// getChildren does not stick on stale EACCES after a TTL-expired permission
// grant.
// ============================================================================

CO_TEST(RestrictedTreeInode, getChildren_returnsEntriesOnUnrestrictedRoot) {
  // Smoke test for the success path: recheck short-circuits on a
  // non-restricted directory and entries are returned without EACCES.
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await rootInode->getChildren(context);
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
// discipline in TreeInode::getChildren that the loadInodes=false default
// never reaches (rlockCheckChild returns an inline VirtualInode for
// unmaterialized entries). loadInodes=true forces every non-loaded entry
// through loadChild, which queues a LoadChildCleanUp drained by SCOPE_EXIT
// after the lock releases.
CO_TEST(RestrictedTreeInode, getChildren_loadInodesTrueExercisesLoadChild) {
  FakeTreeBuilder builder;
  builder.setFile("dir/a.txt", "a");
  builder.setFile("dir/b.txt", "b");
  builder.setFile("dir/c.txt", "c");
  TestMount testMount{builder};

  auto dirInode = testMount.getTreeInode("dir"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await dirInode->getChildren(context, /*loadInodes=*/true);
  EXPECT_EQ(results.size(), 3);
  for (auto& [name, tryVi] : results) {
    CO_ASSERT_TRUE(tryVi.hasValue());
    EXPECT_TRUE(tryVi.value().asInodePtr() != nullptr);
  }
}

// Exercises the EACCES-on-lockContentsWrite path. The SCOPE_EXIT is already
// registered when lockContentsWrite() throws — this verifies the empty
// inodeLoadCleanUps unwind branch is benign.
CO_TEST(RestrictedTreeInode, getChildren_restrictedThrowsEACCES) {
  FakeTreeBuilder builder;
  builder.setFile("dir/file.txt", "content");
  TestMount testMount{builder};

  auto restricted = makeRestrictedInode(testMount, "restricted"_pc);
  auto context = ObjectFetchContext::getNullContext();

  auto result = co_await folly::coro::co_awaitTry(
      restricted->getChildren(context, /*loadInodes=*/false));
  CO_ASSERT_TRUE(result.hasException());
  auto* err = result.tryGetExceptionObject<std::system_error>();
  CO_ASSERT_NE(err, nullptr);
  EXPECT_EQ(err->code().value(), EACCES);
}

// ============================================================================
// acl:restricted-content-mode = omitted
//
// These tests pin the CURRENT behavior under omitted mode: restricted ACL
// roots still appear in enumeration. The FIXME assertions are flipped by the
// diff that implements omission.
// ============================================================================

TEST(RestrictedTreeInode, omittedMode_objectStoreSnapshotsMode) {
  auto testMount = makeOmittedModeTestMount();

  EXPECT_EQ(
      RestrictedContentMode::Omitted,
      testMount->getEdenMount()->getObjectStore()->getRestrictedContentMode());
}

CO_TEST(
    RestrictedTreeInode,
    omittedMode_getChildrenStillIncludesRestrictedChild) {
  auto testMount = makeOmittedModeTestMount();

  auto parentInode = testMount->getTreeInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto children =
      co_await parentInode->getChildren(context, /*loadInodes=*/false);

  auto iter =
      std::find_if(children.begin(), children.end(), [](const auto& entry) {
        return entry.first == "restricted_child"_pc;
      });
  // FIXME: omitted mode should omit restricted roots from child enumeration;
  // they are currently still returned. The next diff implements omission and
  // flips this assertion.
  EXPECT_NE(iter, children.end());
}

CO_TEST(
    RestrictedTreeInode,
    omittedMode_coGetChildrenStillIncludesRestrictedChild) {
  auto testMount = makeOmittedModeTestMount();

  auto edenMount = testMount->getEdenMount();
  auto vi = testMount->getVirtualInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();

  auto results = co_await vi.getChildren(
      "parent"_relpath, edenMount->getObjectStore(), context);
  bool sawRestrictedChild = false;
  for (auto& [name, tryVi] : results) {
    if (name == "restricted_child"_pc) {
      sawRestrictedChild = true;
    }
  }
  // FIXME: omitted mode should hide restricted roots from
  // VirtualInode::getChildren; they are currently still listed. The next
  // diff implements omission and flips this assertion.
  EXPECT_TRUE(sawRestrictedChild);
}

CO_TEST(
    RestrictedTreeInode,
    omittedMode_coGetChildrenAttributesStillIncludesRestrictedChild) {
  auto testMount = makeOmittedModeTestMount();

  auto edenMount = testMount->getEdenMount();
  auto vi = testMount->getVirtualInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto attrs = ENTRY_ATTRIBUTE_SOURCE_CONTROL_TYPE;

  auto results = co_await vi.co_getChildrenAttributes(
      attrs,
      RelativePath{"parent"},
      edenMount->getObjectStore(),
      edenMount->getLastCheckoutTime().toTimespec(),
      context);
  bool sawRestrictedChild = false;
  for (auto& [name, tryAttrs] : results) {
    if (name == "restricted_child"_pc) {
      sawRestrictedChild = true;
    }
  }
  // FIXME: omitted mode should hide restricted roots from
  // co_getChildrenAttributes; they are currently still listed. The next diff
  // implements omission and flips this assertion.
  EXPECT_TRUE(sawRestrictedChild);
}

#ifndef _WIN32
TEST(RestrictedTreeInode, omittedMode_fuseReaddirStillListsRestrictedChild) {
  auto testMount = makeOmittedModeTestMount();

  auto parentInode = testMount->getTreeInode("parent"_relpath);
  auto result =
      parentInode
          ->fuseReaddir(
              FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  auto iter = std::find_if(result.begin(), result.end(), [](const auto& entry) {
    return entry.name == "restricted_child";
  });
  // FIXME: omitted mode should hide restricted roots from readdir; they are
  // currently still listed. The next diff implements omission and flips this
  // assertion.
  EXPECT_NE(iter, result.end());
}
#endif

TEST(RestrictedTreeInode, omittedMode_explicitLookupStillReturnsEACCES) {
  // Regression guard for the omission diff: omitted mode must only affect
  // enumeration. Explicit lookup by name still resolves the restricted child,
  // and reading its contents still fails with EACCES.
  auto testMount = makeOmittedModeTestMount();

  auto parentInode = testMount->getTreeInode("parent"_relpath);
  auto context = ObjectFetchContext::getNullContext();
  auto child =
      parentInode
          ->getOrFindChild("restricted_child"_pc, context, /*loadInodes=*/false)
          .get();
  EXPECT_TRUE(child.isRestricted());

  auto restrictedInode =
      testMount->getTreeInode("parent/restricted_child"_relpath);
  testMount->getBackingStore()->setCheckPermissionResult(
      restrictedInode->getObjectId().value(), false);
  expectEacces([&] {
    restrictedInode->getOrFindChild("secret.txt"_pc, context, false).get();
  });
}
