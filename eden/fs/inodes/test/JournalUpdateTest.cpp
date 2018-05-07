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
  mount_.addFile("new_file.txt", "");
  mount_.move("new_file.txt", "new_file2.txt");

  auto journal = mount_.getEdenMount()->getJournal();
  auto latestDelta = journal.getLatest();
  auto mergedDelta = latestDelta->merge();

  EXPECT_EQ(
      mergedDelta->changedFilesInOverlay, std::unordered_set<RelativePath>{});
  auto dotEdenDirectoryAndNewFile =
      std::unordered_set<RelativePath>{RelativePath{".eden"},
                                       RelativePath{".eden/client"},
                                       RelativePath{".eden/root"},
                                       RelativePath{".eden/socket"},
                                       RelativePath{"new_file2.txt"}};
  EXPECT_EQ(mergedDelta->createdFilesInOverlay, dotEdenDirectoryAndNewFile);
  EXPECT_EQ(
      mergedDelta->removedFilesInOverlay, std::unordered_set<RelativePath>{});
  EXPECT_EQ(mergedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}

TEST_F(JournalUpdateTest, moveFileReplace) {
  mount_.addFile("new_file.txt", "");
  mount_.move("new_file.txt", "existing_file.txt");
  mount_.deleteFile("existing_file.txt");

  auto journal = mount_.getEdenMount()->getJournal();
  auto latestDelta = journal.getLatest();
  auto mergedDelta = latestDelta->merge();

  EXPECT_EQ(
      mergedDelta->changedFilesInOverlay, std::unordered_set<RelativePath>{});
  auto dotEdenDirectory =
      std::unordered_set<RelativePath>{RelativePath{".eden"},
                                       RelativePath{".eden/client"},
                                       RelativePath{".eden/root"},
                                       RelativePath{".eden/socket"}};
  EXPECT_EQ(mergedDelta->createdFilesInOverlay, dotEdenDirectory);
  EXPECT_EQ(
      mergedDelta->removedFilesInOverlay,
      std::unordered_set<RelativePath>{RelativePath{"existing_file.txt"}});
  EXPECT_EQ(mergedDelta->uncleanPaths, std::unordered_set<RelativePath>{});
}
