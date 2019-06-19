/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/TreeInode.h"

#include <folly/Exception.h>
#include <folly/Random.h>
#include <folly/executors/ManualExecutor.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <gflags/gflags.h>
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/utils/FaultInjector.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

static DirEntry makeDirEntry() {
  return DirEntry{S_IFREG | 0644, 1_ino, Hash{}};
}

static TreeEntry makeTreeEntry(folly::StringPiece name) {
  return TreeEntry{Hash{}, name, TreeEntryType::REGULAR_FILE};
}

TEST(TreeInode, findEntryDifferencesWithSameEntriesReturnsNone) {
  DirContents dir;
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one"), makeTreeEntry("two")}};

  EXPECT_FALSE(findEntryDifferences(dir, tree));
}

TEST(TreeInode, findEntryDifferencesReturnsAdditionsAndSubtractions) {
  DirContents dir;
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one"), makeTreeEntry("three")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three", "- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneSubtraction) {
  DirContents dir;
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{{makeTreeEntry("one")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"- two"}), *differences);
}

TEST(TreeInode, findEntryDifferencesWithOneAddition) {
  DirContents dir;
  dir.emplace("one"_pc, makeDirEntry());
  dir.emplace("two"_pc, makeDirEntry());
  Tree tree{
      {makeTreeEntry("one"), makeTreeEntry("two"), makeTreeEntry("three")}};

  auto differences = findEntryDifferences(dir, tree);
  EXPECT_TRUE(differences);
  EXPECT_EQ((std::vector<std::string>{"+ three"}), *differences);
}

TEST(TreeInode, readdirReturnsSelfAndParentBeforeEntries) {
  // libfuse's documentation says returning . and .. is optional, but the FUSE
  // kernel module does not synthesize them, so not returning . and .. would be
  // a visible behavior change relative to a native filesystem.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

  auto root = mount.getEdenMount()->getRootInode();
  auto result = root->readdir(DirList{4096}, 0).extract();

  ASSERT_EQ(4, result.size());
  EXPECT_EQ(".", result[0].name);
  EXPECT_EQ("..", result[1].name);
  EXPECT_EQ("file", result[2].name);
  EXPECT_EQ(".eden", result[3].name);
}

TEST(TreeInode, readdirOffsetsAreNonzero) {
  // readdir's offset parameter means "start here". 0 means start from the
  // beginning. To start after a particular entry, the offset given must be that
  // entry's offset. Therefore, no entries should have offset 0.
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

  auto root = mount.getEdenMount()->getRootInode();
  auto result = root->readdir(DirList{4096}, 0).extract();
  ASSERT_EQ(4, result.size());
  for (auto& entry : result) {
    EXPECT_NE(0, entry.offset);
  }
}

TEST(TreeInode, readdirRespectsOffset) {
  FakeTreeBuilder builder;
  builder.setFiles({{"file", ""}});
  TestMount mount{builder};

  auto root = mount.getEdenMount()->getRootInode();

  const auto resultA = root->readdir(DirList{4096}, 0).extract();
  ASSERT_EQ(4, resultA.size());
  EXPECT_EQ(".", resultA[0].name);
  EXPECT_EQ("..", resultA[1].name);
  EXPECT_EQ("file", resultA[2].name);
  EXPECT_EQ(".eden", resultA[3].name);

  const auto resultB =
      root->readdir(DirList{4096}, resultA[0].offset).extract();
  ASSERT_EQ(3, resultB.size());
  EXPECT_EQ("..", resultB[0].name);
  EXPECT_EQ("file", resultB[1].name);
  EXPECT_EQ(".eden", resultB[2].name);

  const auto resultC =
      root->readdir(DirList{4096}, resultB[0].offset).extract();
  ASSERT_EQ(2, resultC.size());
  EXPECT_EQ("file", resultC[0].name);
  EXPECT_EQ(".eden", resultC[1].name);

  const auto resultD =
      root->readdir(DirList{4096}, resultC[0].offset).extract();
  ASSERT_EQ(1, resultD.size());
  EXPECT_EQ(".eden", resultD[0].name);

  const auto resultE =
      root->readdir(DirList{4096}, resultD[0].offset).extract();
  EXPECT_EQ(0, resultE.size());
}

TEST(TreeInode, readdirIgnoresWildOffsets) {
  TestMount mount{FakeTreeBuilder{}};

  auto root = mount.getEdenMount()->getRootInode();

  auto result = root->readdir(DirList{4096}, 0xfaceb00c).extract();
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
    auto result =
        root->readdir(DirList{kDirListBufferSize}, lastOffset).extract();
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
            root->symlink(randomName(), "symlink-target");
            break;
          case 1: { // unlink
            root->unlink(pickName()).get(0ms);
            break;
          }
          case 2: { // rename
            root->rename(pickName(), root, pickName()).get(0ms);
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
    // If modified, it is not guaranteed to be returned by readdir.
    if (modified.count(name)) {
      continue;
    }

    EXPECT_EQ(1, seen[name])
        << "unmodified entries should be returned by readdir exactly once, but "
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

TEST(TreeInode, create) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};

  // Test creating a new file
  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto createResult = somedir->create("newfile.txt"_pc, S_IFREG | 0740, 0)
                          .getTryVia(mount.getServerExecutor().get());

  EXPECT_TRUE(createResult.hasValue());
  EXPECT_EQ(
      mount.getFileInode("somedir/newfile.txt"_relpath),
      createResult.value().inode);
  EXPECT_FILE_INODE(createResult.value().inode, "", 0740);
}

TEST(TreeInode, createExists) {
  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};

  // Test creating a new file
  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto createResult = folly::makeFutureWith([&] {
                        return somedir->create("foo.txt"_pc, S_IFREG | 0600, 0);
                      })
                          .getTryVia(mount.getServerExecutor().get());

  EXPECT_THROW_ERRNO(createResult.value(), EEXIST);
  EXPECT_FILE_INODE(
      mount.getFileInode("somedir/foo.txt"_relpath), "test\n", 0644);
}

DECLARE_bool(enable_fault_injection);

TEST(TreeInode, createOverlayWriteError) {
  // TODO: We should make TestMount always enable fault injection.
  // Ideally I plan to refactor ServerState soon so that this is configurable in
  // its constructor rather than only controlled through a command line flag.
  FLAGS_enable_fault_injection = true;

  FakeTreeBuilder builder;
  builder.setFile("somedir/foo.txt", "test\n");
  TestMount mount{builder};
  mount.getServerState()->getFaultInjector().injectError(
      "createInodeSaveOverlay",
      "newfile.txt",
      folly::makeSystemErrorExplicit(ENOSPC, "too many cat videos"));

  auto somedir = mount.getTreeInode("somedir"_relpath);
  auto createResult =
      folly::makeFutureWith(
          [&] { return somedir->create("newfile.txt"_pc, S_IFREG | 0600, 0); })
          .getTryVia(mount.getServerExecutor().get());

  EXPECT_THROW_ERRNO(createResult.value(), ENOSPC);
}
