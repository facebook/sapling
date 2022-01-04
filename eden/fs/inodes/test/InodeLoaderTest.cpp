/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeLoader.h"
#include <folly/Exception.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

TEST(InodeLoader, load) {
  FakeTreeBuilder builder;
  builder.setFiles({{"dir/a.txt", ""}, {"dir/sub/b.txt", ""}});
  TestMount mount(builder);

  auto rootInode = mount.getTreeInode(RelativePathPiece());

  {
    auto results =
        collectAll(
            applyToInodes(
                rootInode,
                std::vector<std::string>{
                    "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
                [](InodePtr inode) { return inode->getPath(); },
                ObjectFetchContext::getNullContext()))
            .get();

    EXPECT_EQ("dir/a.txt"_relpath, results[0].value());
    EXPECT_THROW_ERRNO(results[1].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_EQ("dir/sub/b.txt"_relpath, results[3].value());
  }

  {
    auto results =
        collectAll(applyToInodes(
                       rootInode,
                       std::vector<std::string>{
                           "dir/sub/b.txt",
                           "dir/a.txt",
                           "not/exist/a",
                           "not/exist/b",
                           "dir/sub/b.txt"},
                       [](InodePtr inode) { return inode->getPath(); },
                       ObjectFetchContext::getNullContext()))
            .get();

    EXPECT_EQ("dir/sub/b.txt"_relpath, results[0].value());
    EXPECT_EQ("dir/a.txt"_relpath, results[1].value());
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[3].value(), ENOENT);
    EXPECT_EQ(results[0].value(), results[4].value())
        << "dir/sub/b.txt was requested twice and both entries are the same";
  }

  {
    auto results =
        collectAll(
            applyToInodes(
                rootInode,
                std::vector<std::string>{"dir/a.txt", "/invalid///exist/a"},
                [](InodePtr inode) { return inode->getPath(); },
                ObjectFetchContext::getNullContext()))
            .get();

    EXPECT_EQ("dir/a.txt"_relpath, results[0].value());
    EXPECT_THROW_RE(results[1].value(), std::domain_error, "absolute path");
  }
}

TEST(InodeLoader, notReady) {
  FakeTreeBuilder builder;
  builder.setFiles({{"dir/a.txt", ""}, {"dir/sub/b.txt", ""}});
  TestMount mount(builder, /* startReady= */ false);

  auto rootInode = mount.getTreeInode(RelativePathPiece());

  {
    auto future = collectAll(applyToInodes(
        rootInode,
        std::vector<std::string>{
            "dir/a.txt", "not/exist/a", "not/exist/b", "dir/sub/b.txt"},
        [](InodePtr inode) { return inode->getPath(); },
        ObjectFetchContext::getNullContext()));

    builder.setReady("dir");
    builder.setReady("dir/sub");
    builder.setReady("dir/a.txt");
    builder.setReady("dir/sub/b.txt");

    auto results = future.wait().value();

    EXPECT_EQ("dir/a.txt"_relpath, results[0].value());
    EXPECT_THROW_ERRNO(results[1].value(), ENOENT);
    EXPECT_THROW_ERRNO(results[2].value(), ENOENT);
    EXPECT_EQ("dir/sub/b.txt"_relpath, results[3].value());
  }
}
