/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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

  auto summedDelta = journal.accumulateRange(testStart);

  auto oldPath = RelativePath{"new_file.txt"};
  auto newPath = RelativePath{"new_file2.txt"};

  ASSERT_EQ(1, summedDelta->changedFilesInOverlay.count(oldPath));
  ASSERT_EQ(1, summedDelta->changedFilesInOverlay.count(newPath));

  EXPECT_FALSE(summedDelta->changedFilesInOverlay[oldPath].existedBefore);
  EXPECT_FALSE(summedDelta->changedFilesInOverlay[oldPath].existedAfter);
  EXPECT_FALSE(summedDelta->changedFilesInOverlay[newPath].existedBefore);
  EXPECT_TRUE(summedDelta->changedFilesInOverlay[newPath].existedAfter);

  EXPECT_EQ(summedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}

TEST_F(JournalUpdateTest, moveFileReplace) {
  auto& journal = mount_.getEdenMount()->getJournal();
  auto testStart = journal.getLatest()->toSequence;

  mount_.addFile("new_file.txt", "");
  mount_.move("new_file.txt", "existing_file.txt");
  mount_.deleteFile("existing_file.txt");

  auto summedDelta = journal.accumulateRange(testStart);

  auto oldPath = RelativePath{"existing_file.txt"};
  auto newPath = RelativePath{"new_file.txt"};

  ASSERT_EQ(1, summedDelta->changedFilesInOverlay.count(oldPath));
  ASSERT_EQ(1, summedDelta->changedFilesInOverlay.count(newPath));

  EXPECT_TRUE(summedDelta->changedFilesInOverlay[oldPath].existedBefore);
  EXPECT_FALSE(summedDelta->changedFilesInOverlay[oldPath].existedAfter);
  EXPECT_FALSE(summedDelta->changedFilesInOverlay[newPath].existedBefore);
  EXPECT_FALSE(summedDelta->changedFilesInOverlay[newPath].existedAfter);

  EXPECT_EQ(summedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}
