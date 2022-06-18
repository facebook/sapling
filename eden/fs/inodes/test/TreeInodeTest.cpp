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
#include <folly/portability/GFlags.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <optional>
#ifdef _WIN32
#include "eden/fs/prjfs/Enumerator.h"
#else
#include "eden/fs/fuse/DirList.h"
#endif // _WIN32
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/FaultInjector.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

namespace {
std::string testHashHex{
    "faceb00c"
    "deadbeef"
    "c00010ff"
    "1badb002"
    "8badf00d"};

ObjectId testHash(testHashHex);

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
      testHash};

  EXPECT_FALSE(findEntryDifferences(dir, tree));
}

TEST(TreeInode, findEntryDifferencesReturnsAdditionsAndSubtractions) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {{makeTreeEntry("one"), makeTreeEntry("three")},
       CaseSensitivity::Sensitive},
      testHash};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three", "- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneSubtraction) {
  DirContents dir(CaseSensitivity::Sensitive);
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{{{makeTreeEntry("one")}, CaseSensitivity::Sensitive}, testHash};

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
      testHash};

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
    return PathComponent{name};
  };

  // Selects a random name from names and adds it to modified, throwing
  // Collision if it's already been used.
  auto pickName = [&]() -> PathComponentPiece {
    const auto& name = names[folly::Random::rand32(names.size())];
    if (modified.count(name)) {
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

  off_t lastOffset = 0;

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
            root->unlink(
                    pickName(),
                    InvalidationRequired::No,
                    ObjectFetchContext::getNullContext())
                .get(0ms);
            break;
          }
          case 2: { // rename
            root->rename(
                    pickName(),
                    root,
                    pickName(),
                    InvalidationRequired::No,
                    ObjectFetchContext::getNullContext())
                .get(0ms);
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
    if (modified.count(name)) {
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

  EXPECT_FALSE(somedir->getContents().rlock()->isMaterialized());
  DesiredMetadata emptyMetadata{};
  somedir->setattr(emptyMetadata, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(somedir->getContents().rlock()->isMaterialized());

  auto oldmetadata = somedir->getMetadata();
  DesiredMetadata sameMetadata{
      std::nullopt,
      oldmetadata.mode,
      oldmetadata.uid,
      oldmetadata.gid,
      oldmetadata.timestamps.atime.toTimespec(),
      oldmetadata.timestamps.mtime.toTimespec()};
  somedir->setattr(sameMetadata, ObjectFetchContext::getNullContext());
  EXPECT_FALSE(somedir->getContents().rlock()->isMaterialized());

  DesiredMetadata newMetadata{
      std::nullopt,
      oldmetadata.mode,
      oldmetadata.uid + 1,
      oldmetadata.gid + 1,
      oldmetadata.timestamps.atime.toTimespec(),
      oldmetadata.timestamps.mtime.toTimespec()};
  somedir->setattr(newMetadata, ObjectFetchContext::getNullContext());
  EXPECT_TRUE(somedir->getContents().rlock()->isMaterialized());
}

TEST(TreeInode, addNewMaterializationsToActivityBuffer) {
  FakeTreeBuilder builder;
  builder.setFiles(
      {{"somedir/sub/foo.txt", "test\n"}, {"dir2/bar.txt", "test 2\n"}});
  TestMount mount{builder};

  auto& buff = mount.getEdenMount()->getActivityBuffer();
  EXPECT_TRUE(buff.has_value());

  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto dir2 = mount.getTreeInode("dir2"_relpath);

  // Test removing an inode
  somedir->getOrLoadChildTree("sub"_pc, ObjectFetchContext::getNullContext())
      .thenValue([somedir](TreeInodePtr&&) {
        return somedir->removeRecursively(
            "sub"_pc,
            InvalidationRequired::No,
            ObjectFetchContext::getNullContext());
      })
      .get(0ms);
  EXPECT_EQ(1, countEventsWithInode(buff.value(), somedir->getNodeId()));

  // Test creating a directory, a file (on both an unmaterialized/materialized
  // parent), and a symlink
  auto newdir =
      somedir->mkdir("newdir"_pc, S_IFREG | 0740, InvalidationRequired::No);
  EXPECT_EQ(1, countEventsWithInode(buff.value(), newdir->getNodeId()));
  auto newfile = newdir->mknod(
      "newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  auto newfile2 = dir2->mknod(
      "newfile2.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);
  auto symlink = newdir->symlink(
      "symlink.txt"_pc, "newfile.txt", InvalidationRequired::No);
  EXPECT_EQ(1, countEventsWithInode(buff.value(), newfile->getNodeId()));
  EXPECT_EQ(1, countEventsWithInode(buff.value(), newfile2->getNodeId()));
  EXPECT_EQ(1, countEventsWithInode(buff.value(), dir2->getNodeId()));
  EXPECT_EQ(1, countEventsWithInode(buff.value(), symlink->getNodeId()));

  // Ensure that directories did not get materialized a second time
  EXPECT_EQ(1, countEventsWithInode(buff.value(), somedir->getNodeId()));
  EXPECT_EQ(1, countEventsWithInode(buff.value(), newdir->getNodeId()));
}

TEST(TreeInode, getOrFindChildren) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  auto somedir = mount.getTreeInode("somedir"_relpath);

  auto result = somedir->getAllEntryNames();

  EXPECT_EQ(1, result.size());
  EXPECT_THAT(result, testing::Contains("foo.txt"_pc));

  somedir->mknod("newfile.txt"_pc, S_IFREG | 0740, 0, InvalidationRequired::No);

  auto result2 = somedir->getAllEntryNames();

  EXPECT_EQ(2, result2.size());
  EXPECT_THAT(result2, testing::Contains("foo.txt"_pc));
  EXPECT_THAT(result2, testing::Contains("newfile.txt"_pc));

  somedir
      ->unlink(
          "foo.txt"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get();

  auto result3 = somedir->getAllEntryNames();

  EXPECT_EQ(1, result3.size());
  EXPECT_THAT(result3, testing::Not(testing::Contains("foo.txt"_pc)));
  EXPECT_THAT(result3, testing::Contains("newfile.txt"_pc));
}

#endif
