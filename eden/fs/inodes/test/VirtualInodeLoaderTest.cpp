/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/VirtualInodeLoader.h"
#include <folly/test/TestUtils.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using namespace std::literals::chrono_literals;

// VirtualInode objects don't currently know or can compute their paths,
// as once you switch from the Inode objects to => DirEntry/Tree/TreeEntry, you
// lose track of the parent object (unlike inodes, which always know their
// parent). Rather than keep paths around just to report them for this test,
// instead we set the file contents to be their own absolute paths, so we can
// compare the hashes instead.
namespace {
#define FILES                          \
  {                                    \
    {"dir/a.txt", "dir/a.txt"}, {      \
      "dir/sub/b.txt", "dir/sub/b.txt" \
    }                                  \
  }
} // namespace

TEST(InodeLoader, load) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    auto resultsFuture = applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [&](VirtualInode inode,
            RelativePath path) -> folly::SemiFuture<Hash20> {
          return inode.getSHA1(path, objectStore, fetchContext).semi();
        },
        objectStore,
        fetchContext);

    auto results = std::move(resultsFuture).get(0ms);
    EXPECT_EQ(Hash20::sha1("dir/a.txt"), results[0].value());
    EXPECT_THROW_ERRNO(results[1].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_EQ(Hash20::sha1("dir/sub/b.txt"), results[3].value());
  }

  {
    auto resultsFuture = applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/sub/b.txt",
            "dir/a.txt",
            "not/exist/a",
            "not/exist/b",
            "dir/sub/b.txt"},
        [&](VirtualInode inode, RelativePath path) {
          return inode.getSHA1(path, objectStore, fetchContext).semi();
        },
        objectStore,
        fetchContext);

    auto results = std::move(resultsFuture).get(0ms);

    EXPECT_EQ(Hash20::sha1("dir/sub/b.txt"), results[0].value());
    EXPECT_EQ(Hash20::sha1("dir/a.txt"), results[1].value());
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[3].value(), ENOENT);
    EXPECT_EQ(results[0].value(), results[4].value())
        << "dir/sub/b.txt was requested twice and both entries are the same";
  }

  {
    auto resultsFuture = applyToVirtualInode(
        rootInode,
        std::vector<std::string>{"dir/a.txt", "/invalid///exist/a"},
        [&](VirtualInode inode, RelativePath path) {
          return inode.getSHA1(path, objectStore, fetchContext).semi();
        },
        objectStore,
        fetchContext);

    auto results = std::move(resultsFuture).get(0ms);
    EXPECT_EQ(Hash20::sha1("dir/a.txt"), results[0].value());
    EXPECT_THROW_RE(results[1].value(), std::domain_error, "absolute path");
  }
}

TEST(InodeLoader, notReady) {
  FakeTreeBuilder builder;
  builder.setFiles(FILES);
  TestMount mount(builder, /* startReady= */ false);

  auto rootInode = mount.getTreeInode(RelativePathPiece());
  auto objectStore = mount.getEdenMount()->getObjectStore();
  auto fetchContext = ObjectFetchContext::getNullContext();

  {
    auto future = applyToVirtualInode(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [&](VirtualInode inode,
            RelativePath path) -> folly::SemiFuture<Hash20> {
          return inode.getSHA1(path, objectStore, fetchContext).semi();
        },
        objectStore,
        fetchContext);

    builder.setReady("dir");
    builder.setReady("dir/sub");
    builder.setReady("dir/a.txt");
    builder.setReady("dir/sub/b.txt");

    mount.drainServerExecutor();
    auto results = std::move(future).get(0ms);

    EXPECT_EQ(Hash20::sha1("dir/a.txt"), results[0].value());
    EXPECT_THROW_ERRNO(results[1].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_EQ(Hash20::sha1("dir/sub/b.txt"), results[3].value());
  }
}
