/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>

#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;

/**
 * Test to verify that various logic in eden/fs/inodes updates the Journal
 * correctly.
 */

class JournalUpdateTest : public ::testing::Test {
 protected:
  void SetUp() override {
    FakeTreeBuilder builder;
    builder.setFiles({
        {"existing_file.txt", "original contents.\n"},
    });
    mount_.initialize(builder);
  }

  TestMount mount_;
};

TEST_F(JournalUpdateTest, moveFileRename) {
  auto& journal = mount_.getEdenMount()->getJournal();
  auto testStart = journal.getLatest()->toSequence;

  mount_.addFile("new_file.txt", "");
  mount_.move("new_file.txt", "new_file2.txt");

  auto mergedDelta = journal.getLatest()->merge(testStart);

  auto oldPath = RelativePath{"new_file.txt"};
  auto newPath = RelativePath{"new_file2.txt"};

  ASSERT_EQ(1, mergedDelta->changedFilesInOverlay.count(oldPath));
  ASSERT_EQ(1, mergedDelta->changedFilesInOverlay.count(newPath));

  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[oldPath].existedBefore);
  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[oldPath].existedAfter);
  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[newPath].existedBefore);
  EXPECT_TRUE(mergedDelta->changedFilesInOverlay[newPath].existedAfter);

  EXPECT_EQ(mergedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}

TEST_F(JournalUpdateTest, moveFileReplace) {
  auto& journal = mount_.getEdenMount()->getJournal();
  auto testStart = journal.getLatest()->toSequence;

  mount_.addFile("new_file.txt", "");
  mount_.move("new_file.txt", "existing_file.txt");
  mount_.deleteFile("existing_file.txt");

  auto mergedDelta = journal.getLatest()->merge(testStart);

  auto oldPath = RelativePath{"existing_file.txt"};
  auto newPath = RelativePath{"new_file.txt"};

  ASSERT_EQ(1, mergedDelta->changedFilesInOverlay.count(oldPath));
  ASSERT_EQ(1, mergedDelta->changedFilesInOverlay.count(newPath));

  EXPECT_TRUE(mergedDelta->changedFilesInOverlay[oldPath].existedBefore);
  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[oldPath].existedAfter);
  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[newPath].existedBefore);
  EXPECT_FALSE(mergedDelta->changedFilesInOverlay[newPath].existedAfter);

  EXPECT_EQ(mergedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}
