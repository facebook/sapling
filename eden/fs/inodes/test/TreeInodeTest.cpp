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
#include "eden/fs/inodes/FileInode.h"
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
TEST(TreeInode, fuseReaddirReturnsSelfAndParentBeforeEntries) {
  // libfuse's documentation says returning . and .. is optional, but the FUSE
  // kernel module does not synthesize them, so not returning . and .. would be
  // a visible behavior change relative to a native filesystem.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

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

TEST(TreeInode, fuseReaddirOffsetsAreNonzero) {
  // fuseReaddir's offset parameter means "start here". 0 means start from the
  // beginning. To start after a particular entry, the offset given must be that
  // entry's offset. Therefore, no entries should have offset 0.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

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

TEST(TreeInode, fuseReaddirRespectsOffset) {
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

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

TEST(TreeInode, fuseReaddirIgnoresWildOffsets) {
  TestMount mount{FakeTreeBuilder{}};

  auto root = mount.getEdenMount()->getRootInode();

  auto result = root->fuseReaddir(
                        FuseDirList{4096},
                        0xfaceb00c,
                        ObjectFetchContext::getNullContext())
                    .extract();
  EXPECT_EQ(0, result.size());
}

TEST(TreeInode, nfsReaddirEofIsCorrect) {
  FakeTreeBuilder builder;
  builder.setFiles({{"foo", ""}, {"bar", ""}, {"baz", ""}});
  TestMount mount{builder};

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
    const std::vector<std::string>& names) {
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

TEST(TreeInode, fuzzConcurrentModificationAndReaddir) {
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
    runConcurrentModificationAndReaddirIteration(names);
    ++iterations;
  }
  std::cout << "Ran " << iterations << " iterations" << std::endl;
}
#endif

TEST(TreeInode, create) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};

  // Test creating a new file
  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto resultInode = somedir->mknod(
      "newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);

  EXPECT_EQ(mount.getFileInode("somedir/newfile.txt"_relpath), resultInode);

#ifndef _WIN32 // getPermissions are not a part of Inode on Windows
  EXPECT_FILE_INODE(resultInode, "", 0740);
#endif
}

TEST(TreeInode, createExists) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};

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

TEST(TreeInode, createOverlayWriteError) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
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

TEST(TreeInode, removeRecursively) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "foo\n");
  builder.setFile("somedir/bar.txt", "bar\n");
  builder.setFile("somedir/baz.txt", "baz\n");
  builder.setFile("somedir/otherdir/foo.txt", "test\n");
  TestMount mount{builder};

  auto root = mount.getEdenMount()->getRootInode();
  root->removeRecursively(
          "somedir"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_THROW_ERRNO(mount.getTreeInode("somedir"_relpath), ENOENT);
}

TEST(TreeInode, removeRecursivelyNotReady) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "foo\n");
  builder.setFile("somedir/bar.txt", "bar\n");
  builder.setFile("somedir/baz.txt", "baz\n");
  builder.setFile("somedir/otherdir/foo.txt", "test\n");
  TestMount mount;
  mount.initialize(builder, false);

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

TEST(TreeInode, setattr) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
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

TEST(TreeInode, addNewMaterializationsToInodeTraceBus) {
  folly::UnboundedQueue<InodeTraceEvent, true, true, false> queue;
  FakeTreeBuilder builder;
  builder.setFiles(
      {{"somedir/sub/foo.txt", "test\n"}, {"dir2/bar.txt", "test 2\n"}});
  TestMount mount{builder};
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

TEST(TreeInode, getOrFindChildrenSimple) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  auto somedir = mount.getTreeInode("somedir"_relpath);

  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), false);
  EXPECT_EQ(1, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST(TreeInode, getOrFindChildrenLoadInodes) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/bar.txt", "test\n");
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  auto somedir = mount.getTreeInode("somedir"_relpath);

  somedir->unloadChildrenNow();
  auto result =
      somedir->getChildren(ObjectFetchContext::getNullContext(), true);

  EXPECT_EQ(2, result.size());
  EXPECT_THAT(result, testing::Contains(testing::Key("bar.txt"_pc)));
  EXPECT_THAT(result, testing::Contains(testing::Key("foo.txt"_pc)));
  collectResults(mount, std::move(result));
}

TEST(TreeInode, getOrFindChildrenMaterializedLoadedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
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

TEST(TreeInode, getOrFindChildrenMaterializedUnloadedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  builder.setFile("somedir/zoo.txt", "test\n");
  TestMount mount{builder};
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

TEST(TreeInode, getOrFindChildrenRemovedChild) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
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

TEST(TreeInode, if_readdir_prefetching_is_disabled_aux_data_is_not_fetched) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
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

TEST(TreeInode, readdir_does_not_prefetch) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
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

TEST(TreeInode, stat_on_child_does_not_prefetch_parent) {
  FakeTreeBuilder builder;
  auto barObjectId = ObjectId::sha1("bar");
  builder.setFile(
      "foo/bar.txt"_relpath,
      "bar",
      /*executable=*/false,
      /*objectId=*/barObjectId);
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
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

TEST(TreeInode, readdir_followed_by_stat_on_child_prefetches_parents_children) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar.txt", "bar");
  builder.setFile("foo/baz.txt", "baz");
  TestMount mount{builder};
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

TEST(TreeInode, stat_on_directories_only_prefetches_subdirectories) {
  FakeTreeBuilder builder;
  builder.setFile("foo/bar/internal.txt", "internal");
  builder.setFile("foo/baz.txt", "baz");
  builder.setFile("foo/qux/another.txt", "another");
  builder.setFile("foo/dingo.txt", "dingo");
  TestMount mount{builder};
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

#endif // _WIN32
