/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/TreeInode.h"

#include <folly/Exception.h>
#include <folly/Random.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <optional>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/fs/fuse/FuseDirList.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/nfs/NfsDirList.h"
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {
constexpr auto kFutureTimeout = 10s;
constexpr auto materializationTimeoutLimit = 1000ms;

std::string testIdHex{
    "faceb00c"
    "deadbeef"
    "c00010ff"
    "1badb002"
    "8badf00d"};

ObjectId testId(testIdHex);

DirEntry makeDirEntry() {
  return DirEntry{S_IFREG | 0644, 1_ino, ObjectId{}};
}

Tree::value_type makeTreeEntry(folly::StringPiece name) {
  return {
      PathComponent{name}, TreeEntry{ObjectId{}, TreeEntryType::REGULAR_FILE}};
}
} // namespace

class TreeInodeTestBase : public ::testing::TestWithParam<bool> {
 protected:
  void maybeEnableCoroutines(TestMount& mount) {
    if (GetParam()) {
      enableCoroutinesConfig(mount);
    }
  }
};

TEST(TreeInode, findEntryDifferencesWithSameEntriesReturnsNone) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {{makeTreeEntry("one"), makeTreeEntry("two")},
       CaseSensitivity::Sensitive},
      testId};

  EXPECT_FALSE(findEntryDifferences(dir, tree));
}

TEST(TreeInode, findEntryDifferencesReturnsAdditionsAndSubtractions) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {{makeTreeEntry("one"), makeTreeEntry("three")},
       CaseSensitivity::Sensitive},
      testId};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three", "- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneSubtraction) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{{{makeTreeEntry("one")}, CaseSensitivity::Sensitive}, testId};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneAddition) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {{makeTreeEntry("one"), makeTreeEntry("two"), makeTreeEntry("three")},
       CaseSensitivity::Sensitive},
      testId};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three"}), *differences);
}

#ifndef _WIN32
TEST_P(TreeInodeTestBase, fuseReaddirReturnsSelfAndParentBeforeEntries) {
  // libfuse's documentation says returning . and .. is optional, but the FUSE
  // kernel module does not synthesize them, so not returning . and .. would be
  // a visible behavior change relative to a native filesystem.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();
  auto result =
      root->fuseReaddir(
              FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  ASSERT_EQ(4, result.size());
  EXPECT_EQ(".", result[0].name);
  EXPECT_EQ("..", result[1].name);
  EXPECT_EQ("file", result[2].name);
  EXPECT_EQ(".eden", result[3].name);
}

TEST_P(TreeInodeTestBase, fuseReaddirOffsetsAreNonzero) {
  // fuseReaddir's offset parameter means "start here". 0 means start from the
  // beginning. To start after a particular entry, the offset given must be that
  // entry's offset. Therefore, no entries should have offset 0.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();
  auto result =
      root->fuseReaddir(
              FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();
  ASSERT_EQ(4, result.size());
  for (auto& entry : result) {
    EXPECT_NE(0, entry.offset);
  }
}

TEST_P(TreeInodeTestBase, fuseReaddirRespectsOffset) {
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();

  const auto resultA =
      root->fuseReaddir(
              FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();
  ASSERT_EQ(4, resultA.size());
  EXPECT_EQ(".", resultA[0].name);
  EXPECT_EQ("..", resultA[1].name);
  EXPECT_EQ("file", resultA[2].name);
  EXPECT_EQ(".eden", resultA[3].name);

  const auto resultB = root->fuseReaddir(
                               FuseDirList{4096},
                               resultA[0].offset,
                               ObjectFetchContext::getNullContext())
                           .extract();
  ASSERT_EQ(3, resultB.size());
  EXPECT_EQ("..", resultB[0].name);
  EXPECT_EQ("file", resultB[1].name);
  EXPECT_EQ(".eden", resultB[2].name);

  const auto resultC = root->fuseReaddir(
                               FuseDirList{4096},
                               resultB[0].offset,
                               ObjectFetchContext::getNullContext())
                           .extract();
  ASSERT_EQ(2, resultC.size());
  EXPECT_EQ("file", resultC[0].name);
  EXPECT_EQ(".eden", resultC[1].name);

  const auto resultD = root->fuseReaddir(
                               FuseDirList{4096},
                               resultC[0].offset,
                               ObjectFetchContext::getNullContext())
                           .extract();
  ASSERT_EQ(1, resultD.size());
  EXPECT_EQ(".eden", resultD[0].name);

  const auto resultE = root->fuseReaddir(
                               FuseDirList{4096},
                               resultD[0].offset,
                               ObjectFetchContext::getNullContext())
                           .extract();
  EXPECT_EQ(0, resultE.size());
}

TEST_P(TreeInodeTestBase, fuseReaddirIgnoresWildOffsets) {
  TestMount mount{FakeTreeBuilder{}};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();

  auto result = root->fuseReaddir(
                        FuseDirList{4096},
                        0xfaceb00c,
                        ObjectFetchContext::getNullContext())
                    .extract();
  EXPECT_EQ(0, result.size());
}

TEST_P(TreeInodeTestBase, nfsReaddirEofIsCorrect) {
  FakeTreeBuilder builder;
  builder.setFiles({{"foo", ""}, {"bar", ""}, {"baz", ""}});
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();

  uint32_t kMaxCount = 4096;

  // Assert correct EOF behavior across a range of buffer sizes, including when
  // we have only enough buffer space to return all but the final directory
  // entry.
  std::unordered_set<size_t> listingSizesReturned{};
  for (uint32_t bufSize = kNfsDirListInitialOverhead; bufSize <= kMaxCount;
       ++bufSize) {
    auto [list, isEof] = root->nfsReaddir(
        NfsDirList{bufSize, nfsv3Procs::readdir},
        0,
        ObjectFetchContext::getNullContext());

    auto listingSize = list.extractList<entry3>().list.size();
    listingSizesReturned.insert(listingSize);

    // We should return EOF iff we're returning the entire directory listing,
    // which has six entries: ".", "..", ".eden", "foo", "bar", and "baz".
    ASSERT_EQ(listingSize == 6, isEof);
  }

  // To prevent a regression of S298201 we especially want to cover the case
  // where there's exactly one more directory entry we couldn't fit into the
  // response.  If we know that we've returned at least one response without an
  // entire directory listing (buffer too small), and at least one with an
  // entire listing (buffer big enough) while iterating over buffer sizes above,
  // then we know we've covered that case.
  ASSERT_NE(listingSizesReturned.contains(5), 0);
  ASSERT_NE(listingSizesReturned.contains(6), 0);
}

namespace {

// 500 is big enough for ~9 entries
constexpr size_t kDirListBufferSize = 500;
constexpr size_t kDirListNameSize = 25;
constexpr unsigned kModificationCountPerIteration = 4;

void runConcurrentModificationAndReaddirIteration(
    const std::vector<std::string>& names,
    bool useCoroutines) {
  std::unordered_set<std::string> modified;

  struct Collision : std::exception {};

  auto randomName = [&]() -> PathComponent {
    // + 1 to avoid collisions with existing names.
    std::array<char, kDirListNameSize + 1> name;
    for (char& c : name) {
      c = folly::Random::rand32('a', 'z' + 1);
    }
    return PathComponent{std::string_view{name.data(), name.size()}};
  };

  // Selects a random name from names and adds it to modified, throwing
  // Collision if it's already been used.
  auto pickName = [&]() -> PathComponentPiece {
    const auto& name = names[folly::Random::rand32(names.size())];
    if (modified.contains(name)) {
      throw Collision{};
    }
    modified.insert(name);
    // Returning PathComponentPiece is safe because name is a reference into
    // names.
    return PathComponentPiece{name};
  };

  FakeTreeBuilder builder;
  for (const auto& name : names) {
    builder.setFile(name, name);
  }
  TestMount mount{builder};
  if (useCoroutines) {
    enableCoroutinesConfig(mount);
  }
  auto root = mount.getEdenMount()->getRootInode();

  FileOffset lastOffset = 0;

  std::unordered_map<std::string, unsigned> seen;

  for (;;) {
    auto result = root->fuseReaddir(
                          FuseDirList{kDirListBufferSize},
                          lastOffset,
                          ObjectFetchContext::getNullContext())
                      .extract();
    if (result.empty()) {
      break;
    }
    lastOffset = result.back().offset;
    for (auto& entry : result) {
      ++seen[entry.name];
    }

    for (unsigned j = 0; j < kModificationCountPerIteration; ++j) {
      try {
        switch (folly::Random::rand32(3)) {
          case 0: // create
            root->symlink(
                randomName(), "symlink-target", InvalidationRequired::No);
            break;
          case 1: { // unlink
            auto fut = root->unlink(
                               pickName(),
                               InvalidationRequired::No,
                               ObjectFetchContext::getNullContext())
                           .semi()
                           .via(mount.getServerExecutor().get());
            mount.drainServerExecutor();
            std::move(fut).get(0ms);
            break;
          }
          case 2: { // rename
            auto fut = root->rename(
                               pickName(),
                               root,
                               pickName(),
                               InvalidationRequired::No,
                               ObjectFetchContext::getNullContext())
                           .semi()
                           .via(mount.getServerExecutor().get());
            mount.drainServerExecutor();
            std::move(fut).get(0ms);
            break;
          }
        }
      } catch (const Collision&) {
        // Just skip, no big deal.
      }
    }
  }

  // Verify all unmodified files were read.
  for (auto& name : names) {
    // If modified, it is not guaranteed to be returned by fuseReaddir.
    if (modified.contains(name)) {
      continue;
    }

    EXPECT_EQ(1, seen[name])
        << "unmodified entries should be returned by fuseReaddir exactly once, but "
        << name << " wasn't";
  }
}
} // namespace

TEST_P(TreeInodeTestBase, fuzzConcurrentModificationAndReaddir) {
  std::vector<std::string> names;
  for (char c = 'a'; c <= 'z'; ++c) {
    names.emplace_back(kDirListNameSize, c);
  }

  auto minimumTime = 500ms;
  unsigned minimumIterations = 5;

  auto end = std::chrono::steady_clock::now() + minimumTime;
  unsigned iterations = 0;
  while (std::chrono::steady_clock::now() < end ||
         iterations < minimumIterations) {
    runConcurrentModificationAndReaddirIteration(names, GetParam());
    ++iterations;
  }
  std::cout << "Ran " << iterations << " iterations" << std::endl;
}
#endif

TEST_P(TreeInodeTestBase, create) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Test creating a new file
  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto resultInode = somedir->mknod(
      "newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);

  EXPECT_EQ(mount.getFileInode("somedir/newfile.txt"_relpath), resultInode);

#ifndef _WIN32 // getPermissions are not a part of Inode on Windows
  EXPECT_FILE_INODE(resultInode, "", 0740);
#endif
}

TEST_P(TreeInodeTestBase, createExists) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Test creating a new file
  auto somedir = mount.getTreeInode("somedir"_relpath);

  EXPECT_THROW_ERRNO(
      somedir->mknod("foo.txt"_pc, S_IFREG | 0600, 0, InvalidationRequired::No),
      EEXIST);
#ifndef _WIN32 // getPermissions are not a part of Inode on Windows
  EXPECT_FILE_INODE(
      mount.getFileInode("somedir/foo.txt"_relpath), "test\n", 0644);
#endif
}

#ifndef _WIN32

TEST_P(TreeInodeTestBase, createOverlayWriteError) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.getServerState()->getFaultInjector().injectError(
      "createInodeSaveOverlay",
      "newfile.txt",
      folly::makeSystemErrorExplicit(ENOSPC, "too many cat videos"));

  auto somedir = mount.getTreeInode("somedir"_relpath);

  EXPECT_THROW_ERRNO(
      somedir->mknod(
          "newfile.txt"_pc, S_IFREG | 0600, 0, InvalidationRequired::No),
      ENOSPC);
}

#endif

TEST_P(TreeInodeTestBase, removeRecursively) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "foo\n");
  builder.setFile("somedir/bar.txt", "bar\n");
  builder.setFile("somedir/baz.txt", "baz\n");
  builder.setFile("somedir/otherdir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();
  root->removeRecursively(
          "somedir"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_THROW_ERRNO(mount.getTreeInode("somedir"_relpath), ENOENT);
}

TEST_P(TreeInodeTestBase, removeRecursivelyNotReady) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "foo\n");
  builder.setFile("somedir/bar.txt", "bar\n");
  builder.setFile("somedir/baz.txt", "baz\n");
  builder.setFile("somedir/otherdir/foo.txt", "test\n");
  TestMount mount;
  mount.initialize(builder, false);
  maybeEnableCoroutines(mount);

  auto root = mount.getEdenMount()->getRootInode();
  auto fut = root->getOrLoadChildTree(
                     "somedir"_pc, ObjectFetchContext::getNullContext())
                 .thenValue([root](TreeInodePtr&&) {
                   return root->removeRecursively(
                       "somedir"_pc,
                       InvalidationRequired::No,
                       ObjectFetchContext::getNullContext());
                 });
  EXPECT_FALSE(fut.isReady());

  builder.setAllReady();
  std::move(fut).get(0ms);

  EXPECT_THROW_ERRNO(mount.getTreeInode("somedir"_relpath), ENOENT);
}

#ifndef _WIN32

TEST_P(TreeInodeTestBase, setattr) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);

  EXPECT_FALSE(somedir->isMaterialized());
  DesiredMetadata emptyMetadata{};
  somedir->setattr(emptyMetadata, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(somedir->isMaterialized());

  auto oldauxData = somedir->getMetadata();
  DesiredMetadata sameMetadata{
      std::nullopt,
      oldauxData.mode,
      oldauxData.uid,
      oldauxData.gid,
      oldauxData.timestamps.atime.toTimespec(),
      oldauxData.timestamps.mtime.toTimespec()};
  somedir->setattr(sameMetadata, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(somedir->isMaterialized());

  DesiredMetadata newMetadata{
      std::nullopt,
      oldauxData.mode,
      oldauxData.uid + 1,
      oldauxData.gid + 1,
      oldauxData.timestamps.atime.toTimespec(),
      oldauxData.timestamps.mtime.toTimespec()};
  somedir->setattr(newMetadata, ObjectFetchContext::getNullContext());
  EXPECT_TRUE(somedir->isMaterialized());
}

TEST_P(TreeInodeTestBase, addNewMaterializationsToInodeTraceBus) {
  folly::UnboundedQueue<InodeTraceEvent, true, true, false> queue;
  FakeTreeBuilder builder;
  builder.setFiles(
      {{"somedir/sub/foo.txt", "test\n"}, {"dir2/bar.txt", "test 2\n"}});
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto& trace_bus = mount.getEdenMount()->getInodeTraceBus();

  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto sub = mount.getTreeInode("somedir/sub"_relpath);
  auto dir2 = mount.getTreeInode("dir2"_relpath);

  // Detect inode materialization events and add events to synchronized queue
  auto handle = trace_bus.subscribeFunction(
      fmt::format(
          "treeInodeTest-{}", mount.getEdenMount()->getPath().basename()),
      [&](const InodeTraceEvent& event) {
        if (event.eventType == InodeEventType::MATERIALIZE) {
          queue.enqueue(event);
        }
      });

  // Wait for any initial materialization events to complete
  while (queue.try_dequeue_for(materializationTimeoutLimit).has_value()) {
  };

  // Test removing an inode (in this case a tree inode which also materializes
  // that tree inode before removing it)
  somedir->getOrLoadChildTree("sub"_pc, ObjectFetchContext::getNullContext())
      .thenValue([somedir](TreeInodePtr&&) {
        return somedir->removeRecursively(
            "sub"_pc,
            InvalidationRequired::No,
            ObjectFetchContext::getNullContext());
      })
      .get(0ms);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, sub->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, somedir->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, somedir->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, sub->getNodeId()));

  // Test creating a directory
  auto newdir =
      somedir->mkdir("newdir"_pc, S_IFREG | 0740, InvalidationRequired::No);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, newdir->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, newdir->getNodeId()));

  // Test creating a file (on an already materialized parent)
  auto newfile = newdir->mknod(
      "newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, newfile->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, newfile->getNodeId()));

  // Test creating a file (on an unmaterialized parent)
  auto newfile2 = dir2->mknod(
      "newfile2.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, dir2->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, dir2->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, newfile2->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, newfile2->getNodeId()));

  // Test creating a symlink
  auto symlink = newdir->symlink(
      "symlink.txt"_pc, "newfile.txt", InvalidationRequired::No);
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::START, symlink->getNodeId()));
  EXPECT_TRUE(isInodeMaterializedInQueue(
      queue, InodeEventProgress::END, symlink->getNodeId()));

  // Ensure we do not count any other materializations a second time
  EXPECT_FALSE(queue.try_dequeue_for(materializationTimeoutLimit).has_value());
}

void collectResults(
    TestMount& testMount,
    std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>
        results) {
  testMount.drainServerExecutor();
  for (auto& result : results) {
    std::move(result.second).get(kFutureTimeout);
  }
}

TEST_P(TreeInodeTestBase, getOrFindChildrenSimple) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);

  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), false);
  EXPECT_EQ(1, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST_P(TreeInodeTestBase, getOrFindChildrenLoadInodes) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/bar.txt", "test\n");
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);

  somedir->unloadChildrenNow();
  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), true);

  EXPECT_EQ(2, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("bar.txt"_pc)));
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST_P(TreeInodeTestBase, getOrFindChildrenMaterializedLoadedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);
  somedir->mknod("newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  EXPECT_TRUE(somedir->isMaterialized());

  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), false);

  EXPECT_EQ(2, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  EXPECT_THAT(result, testing::Contains(testing::Key("newfile.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST_P(TreeInodeTestBase, getOrFindChildrenMaterializedUnloadedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  builder.setFile("somedir/zoo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);
  {
    somedir->mknod(
        "newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  }

  somedir->unloadChildrenNow();
  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), false);

  EXPECT_EQ(3, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  EXPECT_THAT(result, testing::Contains(testing::Key("newfile.txt"_pc)));
  EXPECT_THAT(result, testing::Contains(testing::Key("zoo.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST_P(TreeInodeTestBase, getOrFindChildrenRemovedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  auto somedir = mount.getTreeInode("somedir"_relpath);
  somedir->mknod("newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);

  auto fut = somedir
                 ->unlink(
                     "foo.txt"_pc,
                     InvalidationRequired::No,
                     ObjectFetchContext::getNullContext())
                 .semi()
                 .via(mount.getServerExecutor().get());
  mount.drainServerExecutor();
  std::move(fut).get(0ms);

  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), false);

  EXPECT_EQ(1, result.size());
  EXPECT_THAT(
      result, testing::Not(testing::Contains(testing::Key("foo.txt"_pc))));
  EXPECT_THAT(result, testing::Contains(testing::Key("newfile.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST_P(
    TreeInodeTestBase,
    if_readdir_prefetching_is_disabled_aux_data_is_not_fetched) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.updateEdenConfig({
      {"mount:readdir-prefetch", "none"},
  });

  auto foo = mount.getTreeInode("foo"_relpath);
  auto result =
      foo->fuseReaddir(
             FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  ASSERT_EQ(4, result.size());
  EXPECT_EQ(".", result[0].name);
  EXPECT_EQ("..", result[1].name);
  EXPECT_EQ("bar.txt", result[2].name);
  EXPECT_EQ("baz.txt", result[3].name);

  auto bar = mount.getFileInode("foo/bar.txt"_relpath);
  bar->stat(ObjectFetchContext::getNullContext()).get();
  EXPECT_EQ(1, mount.getBackingStore()->getAuxDataLookups().size());

  // Pump the prefetch operation here.
  mount.drainServerExecutor();

  EXPECT_EQ(1, mount.getBackingStore()->getAuxDataLookups().size());
}

TEST_P(TreeInodeTestBase, readdir_does_not_prefetch) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.updateEdenConfig({
      {"mount:readdir-prefetch", "both"},
  });

  auto foo = mount.getTreeInode("foo"_relpath);
  auto result =
      foo->fuseReaddir(
             FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  ASSERT_EQ(4, result.size());
  EXPECT_EQ(".", result[0].name);
  EXPECT_EQ("..", result[1].name);
  EXPECT_EQ("bar.txt", result[2].name);
  EXPECT_EQ("baz.txt", result[3].name);

  mount.drainServerExecutor();

  auto auxData = mount.getBackingStore()->getAuxDataLookups();
  EXPECT_EQ(0, auxData.size());
}

TEST_P(TreeInodeTestBase, stat_on_child_does_not_prefetch_parent) {
  FakeTreeBuilder builder;
  auto barObjectId = ObjectId::sha1("bar");
  builder.setFile(
      "foo/bar.txt"_relpath,
      "bar",
      /*executable=*/false,
      /*objectId=*/barObjectId);
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.updateEdenConfig({
      {"mount:readdir-prefetch", "both"},
  });

  auto bar = mount.getFileInode("foo/bar.txt"_relpath);

  auto executor = mount.getServerExecutor().get();

  // Inject a fault in the getBlobAuxData method because we want to trigger the
  // prefetch logic without actually fetching the aux data for `bar`.
  mount.getServerState()->getFaultInjector().injectBlock(
      "getBlobAuxData", ".*");

  auto statFuture =
      bar->stat(ObjectFetchContext::getNullContext()).semi().via(executor);

  mount.drainServerExecutor();

  EXPECT_FALSE(statFuture.isReady());

  auto auxData = mount.getBackingStore()->getAuxDataLookups();
  EXPECT_EQ(1, auxData.size());
  EXPECT_EQ(auxData.front().getBytes(), barObjectId.getBytes());

  // Unblock stat
  mount.getServerState()->getFaultInjector().unblock("getBlobAuxData", ".*");

  auto waitedStatFuture = std::move(statFuture).waitVia(executor);
  EXPECT_TRUE(waitedStatFuture.isReady());
}

TEST_P(
    TreeInodeTestBase,
    readdir_followed_by_stat_on_child_prefetches_parents_children) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.updateEdenConfig({
      {"mount:readdir-prefetch", "both"},
  });

  auto foo = mount.getTreeInode("foo"_relpath);
  auto result =
      foo->fuseReaddir(
             FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  ASSERT_EQ(4, result.size());
  EXPECT_EQ(".", result[0].name);
  EXPECT_EQ("..", result[1].name);
  EXPECT_EQ("bar.txt", result[2].name);
  EXPECT_EQ("baz.txt", result[3].name);

  auto bar = mount.getFileInode("foo/bar.txt"_relpath);
  bar->stat(ObjectFetchContext::getNullContext()).get();
  EXPECT_EQ(1, mount.getBackingStore()->getAuxDataLookups().size());

  // Pump the prefetch operation here.
  mount.drainServerExecutor();

  EXPECT_EQ(2, mount.getBackingStore()->getAuxDataLookups().size());
}

TEST_P(TreeInodeTestBase, stat_on_directories_only_prefetches_subdirectories) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar/internal.txt", "internal");
  builder.setFile("foo/baz.txt", "baz");
  builder.setFile("foo/qux/another.txt", "another");
  builder.setFile("foo/dingo.txt", "dingo");
  TestMount mount{builder};
  maybeEnableCoroutines(mount);
  mount.updateEdenConfig({
      {"mount:readdir-prefetch", "both"},
  });

  auto foo = mount.getTreeInode("foo"_relpath);
  auto result =
      foo->fuseReaddir(
             FuseDirList{4096}, 0, ObjectFetchContext::getNullContext())
          .extract();

  // stat() a directory. This looks like a `find .` operation where the
  // traversal itself will lookup() any child directories, but will not stat()
  // blobs. Therefore, it's best to only prefetch tree aux data.
  auto bar = mount.getTreeInode("foo/bar"_relpath);
  bar->stat(ObjectFetchContext::getNullContext()).get();
  EXPECT_EQ(0, mount.getBackingStore()->getAuxDataLookups().size());

  // Pump the prefetch operation here.
  mount.drainServerExecutor();

  // Trees don't have blob aux data.
  EXPECT_EQ(0, mount.getBackingStore()->getAuxDataLookups().size());

  // Now stat() a file. We already prefetched directories, so if a file is
  // stat()'d, that's a clue that the rest of the files should be prefetched.

  auto baz = mount.getFileInode("foo/baz.txt"_relpath);
  baz->stat(ObjectFetchContext::getNullContext()).get();
  EXPECT_EQ(1, mount.getBackingStore()->getAuxDataLookups().size());

  // Pump the prefetch operation here.
  mount.drainServerExecutor();

  EXPECT_EQ(2, mount.getBackingStore()->getAuxDataLookups().size());
}

TEST_P(TreeInodeTestBase, buildDirFromTree) {
  // Set up a mount with a known directory structure
  FakeTreeBuilder builder;
  builder.setFiles({
      {"dir/a.txt", "content_a"},
      {"dir/b.txt", "content_b"},
      {"dir/c.txt", "content_c"},
  });
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Load the directory inode — this exercises buildDirFromTree internally
  auto dir = mount.getTreeInode("dir"_relpath);
  auto contents = dir->lockContentsRead();

  // Verify all entries are present
  EXPECT_EQ(3, contents->entries.size());
  EXPECT_NE(contents->entries.end(), contents->entries.find("a.txt"_pc));
  EXPECT_NE(contents->entries.end(), contents->entries.find("b.txt"_pc));
  EXPECT_NE(contents->entries.end(), contents->entries.find("c.txt"_pc));

  // Verify entries are not materialized (they come from source control)
  EXPECT_FALSE(contents->entries.at("a.txt"_pc).isMaterialized());
  EXPECT_FALSE(contents->entries.at("b.txt"_pc).isMaterialized());
  EXPECT_FALSE(contents->entries.at("c.txt"_pc).isMaterialized());

  // Verify each entry has a unique inode number
  auto inoA = contents->entries.at("a.txt"_pc).getInodeNumber();
  auto inoB = contents->entries.at("b.txt"_pc).getInodeNumber();
  auto inoC = contents->entries.at("c.txt"_pc).getInodeNumber();
  EXPECT_NE(inoA, inoB);
  EXPECT_NE(inoA, inoC);
  EXPECT_NE(inoB, inoC);

  // Verify mode bits are correct for regular files
  EXPECT_EQ(S_IFREG | 0644, contents->entries.at("a.txt"_pc).getInitialMode());
}

TEST_P(TreeInodeTestBase, buildDirFromTreePropagatesIsRestricted) {
  FakeTreeBuilder builder;
  builder.setFile("restricted_dir/file.txt", "content");
  builder.setDirIsRestricted("restricted_dir");
  builder.setFile("normal_dir/file.txt", "content");
  TestMount testMount{builder};
  maybeEnableCoroutines(testMount);

  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto restrictedIter = contents->entries.find("restricted_dir"_pc);
  ASSERT_NE(restrictedIter, contents->entries.end());
  EXPECT_TRUE(restrictedIter->second.isDirectory());
  EXPECT_TRUE(restrictedIter->second.isRestricted());

  auto normalIter = contents->entries.find("normal_dir"_pc);
  ASSERT_NE(normalIter, contents->entries.end());
  EXPECT_TRUE(normalIter->second.isDirectory());
  EXPECT_FALSE(normalIter->second.isRestricted());
}

TEST(DirEntry, isRestrictedBitField) {
  DirEntry restrictedEntry(
      S_IFDIR | 0755,
      43_ino,
      ObjectId("def"),
      /*isRestricted=*/true);
  EXPECT_TRUE(restrictedEntry.isRestricted());

  DirEntry defaultEntry(S_IFDIR | 0755, 44_ino, ObjectId("ghi"));
  EXPECT_FALSE(defaultEntry.isRestricted());

  DirEntry materialized(S_IFDIR | 0755, 45_ino);
  EXPECT_FALSE(materialized.isRestricted());

  DirEntry mutableEntry(S_IFDIR | 0755, 46_ino, ObjectId("jkl"));
  EXPECT_FALSE(mutableEntry.isRestricted());
  mutableEntry.setRestricted(true);
  EXPECT_TRUE(mutableEntry.isRestricted());
  mutableEntry.setRestricted(false);
  EXPECT_FALSE(mutableEntry.isRestricted());
}
TEST_P(TreeInodeTestBase, childMaterializedSkipsOverlayWrite) {
  FakeTreeBuilder builder;
  builder.setFiles({
      {"dir/a.txt", "content_a"},
      {"dir/b.txt", "content_b"},
  });
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Materialize "dir" by writing a file — this writes the overlay
  mount.overwriteFile("dir/a.txt", "modified\n");

  auto dir = mount.getTreeInode("dir"_relpath);
  auto dirIno = dir->getNodeId();
  auto* overlay = mount.getEdenMount()->getOverlay();

  // dir is now materialized and overlay has its data.
  // "a.txt" should be materialized, "b.txt" should not.
  ASSERT_TRUE(dir->isMaterialized());
  {
    auto overlayDir = overlay->loadOverlayDir(dirIno);
    EXPECT_TRUE(overlayDir.at("a.txt"_pc).isMaterialized());
    EXPECT_FALSE(overlayDir.at("b.txt"_pc).isMaterialized());
  }

  // Call childMaterialized for "b.txt" with writeOverlay=false
  auto renameLock = mount.getEdenMount()->acquireRenameLock();
  dir->childMaterialized(renameLock, "b.txt"_pc, /*writeOverlay=*/false);

  // In-memory: b.txt is now materialized
  {
    auto contents = dir->lockContentsRead();
    EXPECT_TRUE(contents->entries.at("b.txt"_pc).isMaterialized());
  }

  // On-disk overlay was NOT updated — b.txt should still show as
  // non-materialized in the persisted overlay data
  {
    auto overlayDir = overlay->loadOverlayDir(dirIno);
    EXPECT_FALSE(overlayDir.at("b.txt"_pc).isMaterialized());
  }
}

TEST_P(TreeInodeTestBase, childMaterializedWritesOverlayByDefault) {
  FakeTreeBuilder builder;
  builder.setFiles({
      {"dir/a.txt", "content_a"},
      {"dir/b.txt", "content_b"},
  });
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Materialize "dir" by writing a file
  mount.overwriteFile("dir/a.txt", "modified\n");

  auto dir = mount.getTreeInode("dir"_relpath);
  auto dirIno = dir->getNodeId();
  auto* overlay = mount.getEdenMount()->getOverlay();

  // Call childMaterialized for "b.txt" with default writeOverlay=true
  auto renameLock = mount.getEdenMount()->acquireRenameLock();
  dir->childMaterialized(renameLock, "b.txt"_pc);

  // Both in-memory and overlay should show b.txt as materialized
  {
    auto contents = dir->lockContentsRead();
    EXPECT_TRUE(contents->entries.at("b.txt"_pc).isMaterialized());
  }
  {
    auto overlayDir = overlay->loadOverlayDir(dirIno);
    EXPECT_TRUE(overlayDir.at("b.txt"_pc).isMaterialized());
  }
}

TEST_P(TreeInodeTestBase, childDematerializedSkipsOverlayWrite) {
  FakeTreeBuilder builder;
  builder.setFiles({
      {"dir/a.txt", "content_a"},
      {"dir/b.txt", "content_b"},
  });
  TestMount mount{builder};
  maybeEnableCoroutines(mount);

  // Materialize "dir" and "b.txt" by writing to b.txt
  mount.overwriteFile("dir/b.txt", "modified\n");

  auto dir = mount.getTreeInode("dir"_relpath);
  auto dirIno = dir->getNodeId();
  auto* overlay = mount.getEdenMount()->getOverlay();

  // b.txt should be materialized in both memory and overlay
  ASSERT_TRUE(dir->isMaterialized());
  {
    auto overlayDir = overlay->loadOverlayDir(dirIno);
    EXPECT_TRUE(overlayDir.at("b.txt"_pc).isMaterialized());
  }

  // Call childDematerialized for "b.txt" with writeOverlay=false
  auto renameLock = mount.getEdenMount()->acquireRenameLock();
  dir->childDematerialized(
      renameLock,
      "b.txt"_pc,
      ObjectId{"b_hash"},
      /*writeOverlay=*/false);

  // In-memory: b.txt should now be dematerialized
  {
    auto contents = dir->lockContentsRead();
    EXPECT_FALSE(contents->entries.at("b.txt"_pc).isMaterialized());
  }

  // On-disk overlay was NOT updated — b.txt should still show as
  // materialized in the persisted overlay data
  {
    auto overlayDir = overlay->loadOverlayDir(dirIno);
    EXPECT_TRUE(overlayDir.at("b.txt"_pc).isMaterialized());
  }
}

#endif // _WIN32

TEST_P(TreeInodeTestBase, checkoutPropagatesIsRestricted) {
  // Start with a tree that has no ACL directories.
  FakeTreeBuilder builder1;
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("normal_dir/file.txt", "content");
  TestMount testMount{builder1};
  maybeEnableCoroutines(testMount);

  // Create a second commit that adds an ACL directory.
  auto builder2 = builder1.clone();
  builder2.setFile("acl_dir/file.txt", "acl content");
  builder2.setDirIsRestricted("acl_dir");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit(RootId{"2"}, builder2);
  commit2->setReady();

  auto executor = testMount.getServerExecutor().get();
  auto checkoutResult = testMount.getEdenMount()
                            ->checkout(
                                testMount.getRootInode(),
                                RootId{"2"},
                                ObjectFetchContext::getNullContext(),
                                __func__)
                            .semi()
                            .via(executor);
  testMount.drainServerExecutor();
  ASSERT_TRUE(checkoutResult.isReady());
  auto result = std::move(checkoutResult).get();
  EXPECT_EQ(0, result.conflicts.size());

  // Verify the new acl_dir entry has isRestricted set.
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto aclIter = contents->entries.find("acl_dir"_pc);
  ASSERT_NE(aclIter, contents->entries.end());
  EXPECT_TRUE(aclIter->second.isDirectory());
  EXPECT_TRUE(aclIter->second.isRestricted());
}

TEST_P(TreeInodeTestBase, checkoutRemovesRestrictionWhenAclRemoved) {
  // Start with a tree that has an ACL directory.
  FakeTreeBuilder builder1;
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("acl_dir/file.txt", "acl content");
  builder1.setDirIsRestricted("acl_dir");
  TestMount testMount{builder1};
  maybeEnableCoroutines(testMount);

  // Create a second commit where acl_dir exists but without isRestricted.
  // Build from scratch rather than cloning since clone preserves isRestricted.
  FakeTreeBuilder builder2;
  builder2.setFile("src/main.c", "int main() { return 0; }\n");
  builder2.setFile("acl_dir/file.txt", "acl content modified");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit(RootId{"2"}, builder2);
  commit2->setReady();

  auto executor = testMount.getServerExecutor().get();
  auto checkoutResult = testMount.getEdenMount()
                            ->checkout(
                                testMount.getRootInode(),
                                RootId{"2"},
                                ObjectFetchContext::getNullContext(),
                                __func__)
                            .semi()
                            .via(executor);
  testMount.drainServerExecutor();
  ASSERT_TRUE(checkoutResult.isReady());
  auto result = std::move(checkoutResult).get();
  EXPECT_EQ(0, result.conflicts.size());

  // Verify acl_dir no longer has isRestricted set.
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto aclIter = contents->entries.find("acl_dir"_pc);
  ASSERT_NE(aclIter, contents->entries.end());
  EXPECT_TRUE(aclIter->second.isDirectory());
  EXPECT_FALSE(aclIter->second.isRestricted());
}

TEST_P(TreeInodeTestBase, checkoutAddsRestrictionWhenAclAdded) {
  // Start with a tree where acl_dir does not have isRestricted.
  FakeTreeBuilder builder1;
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("acl_dir/file.txt", "acl content");
  TestMount testMount{builder1};
  maybeEnableCoroutines(testMount);

  // Create a second commit where acl_dir has isRestricted set AND content
  // changes. The checkout code intentionally does not compare isRestricted
  // alone — it only processes entries where the tree content differs. So we
  // must also change a file to trigger checkout processing of acl_dir.
  auto builder2 = builder1.clone();
  builder2.replaceFile("acl_dir/file.txt", "acl content modified");
  builder2.setDirIsRestricted("acl_dir");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit(RootId{"2"}, builder2);
  commit2->setReady();

  auto executor = testMount.getServerExecutor().get();
  auto checkoutResult = testMount.getEdenMount()
                            ->checkout(
                                testMount.getRootInode(),
                                RootId{"2"},
                                ObjectFetchContext::getNullContext(),
                                __func__)
                            .semi()
                            .via(executor);
  testMount.drainServerExecutor();
  ASSERT_TRUE(checkoutResult.isReady());
  auto result = std::move(checkoutResult).get();
  EXPECT_EQ(0, result.conflicts.size());

  // Verify acl_dir now has isRestricted set.
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto aclIter = contents->entries.find("acl_dir"_pc);
  ASSERT_NE(aclIter, contents->entries.end());
  EXPECT_TRUE(aclIter->second.isDirectory());
  EXPECT_TRUE(aclIter->second.isRestricted());
}

namespace {
CheckoutConflict makeConflict(
    ConflictType type,
    folly::StringPiece path,
    folly::StringPiece message = "",
    Dtype dtype = Dtype::UNKNOWN) {
  CheckoutConflict conflict;
  conflict.type() = type;
  conflict.path() = path.str();
  conflict.message() = message.str();
  conflict.dtype() = dtype;
  return conflict;
}
} // namespace

TEST_P(
    TreeInodeTestBase,
    checkoutAddsRestrictionConflictsWithDirtyUnrestrictedDir) {
  // Start with a tree where acl_dir does not have isRestricted.
  FakeTreeBuilder builder1;
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("acl_dir/file.txt", "acl content");
  TestMount testMount{builder1};
  maybeEnableCoroutines(testMount);

  // Materialize acl_dir by writing to a file inside it. This makes
  // acl_dir's subtree "dirty" from the checkout pre-check's perspective.
  testMount.overwriteFile("acl_dir/file.txt", "local modification");

  // Create a second commit where acl_dir has isRestricted set AND content
  // changes, to force checkout to process the acl_dir entry.
  auto builder2 = builder1.clone();
  builder2.replaceFile("acl_dir/file.txt", "acl content modified");
  builder2.setDirIsRestricted("acl_dir");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit(RootId{"2"}, builder2);
  commit2->setReady();

  auto executor = testMount.getServerExecutor().get();
  auto checkoutResult = testMount.getEdenMount()
                            ->checkout(
                                testMount.getRootInode(),
                                RootId{"2"},
                                ObjectFetchContext::getNullContext(),
                                __func__,
                                CheckoutMode::NORMAL)
                            .semi()
                            .via(executor);
  testMount.drainServerExecutor();
  ASSERT_TRUE(checkoutResult.isReady());
  auto result = std::move(checkoutResult).get();

  // Expect the pre-check to surface a single directory-level
  // MODIFIED_MODIFIED conflict at acl_dir and to leave the subtree
  // untouched (no recursive descent on the non-force path).
  EXPECT_THAT(
      result.conflicts,
      ::testing::UnorderedElementsAre(makeConflict(
          ConflictType::MODIFIED_MODIFIED, "acl_dir", "", Dtype::DIR)));

  // Verify acl_dir is still unrestricted — the swap was skipped.
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto aclIter = contents->entries.find("acl_dir"_pc);
  ASSERT_NE(aclIter, contents->entries.end());
  EXPECT_TRUE(aclIter->second.isDirectory());
  EXPECT_FALSE(aclIter->second.isRestricted());
}

TEST_P(
    TreeInodeTestBase,
    checkoutForceAddsRestrictionOverDirtyUnrestrictedDir) {
  // Same dirty setup as the non-force test.
  FakeTreeBuilder builder1;
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("acl_dir/file.txt", "acl content");
  TestMount testMount{builder1};
  maybeEnableCoroutines(testMount);

  testMount.overwriteFile("acl_dir/file.txt", "local modification");

  auto builder2 = builder1.clone();
  builder2.replaceFile("acl_dir/file.txt", "acl content modified");
  builder2.setDirIsRestricted("acl_dir");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit(RootId{"2"}, builder2);
  commit2->setReady();

  // Force mode should report the conflict AND still apply the transition
  // (matches the force-path semantics in processCheckoutEntryImpl).
  auto executor = testMount.getServerExecutor().get();
  auto checkoutResult = testMount.getEdenMount()
                            ->checkout(
                                testMount.getRootInode(),
                                RootId{"2"},
                                ObjectFetchContext::getNullContext(),
                                __func__,
                                CheckoutMode::FORCE)
                            .semi()
                            .via(executor);
  testMount.drainServerExecutor();
  ASSERT_TRUE(checkoutResult.isReady());
  auto result = std::move(checkoutResult).get();

  EXPECT_THAT(
      result.conflicts,
      ::testing::Contains(makeConflict(
          ConflictType::MODIFIED_MODIFIED, "acl_dir", "", Dtype::DIR)));

  // Verify acl_dir is now restricted — force mode drove the swap through.
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto contents = rootInode->lockContentsRead();

  auto aclIter = contents->entries.find("acl_dir"_pc);
  ASSERT_NE(aclIter, contents->entries.end());
  EXPECT_TRUE(aclIter->second.isDirectory());
  EXPECT_TRUE(aclIter->second.isRestricted());
}

INSTANTIATE_TEST_SUITE_P(
    TreeInodeTestVariants,
    TreeInodeTestBase,
    ::testing::Bool(),
    [](const ::testing::TestParamInfo<bool>& info) {
      return info.param ? "Coroutines" : "Futures";
    });
