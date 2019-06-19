/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/journal/Journal.h"
#include <gmock/gmock.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using ::testing::UnorderedElementsAre;

TEST(Journal, merges_chained_deltas) {
  Journal journal;

  // Make an initial entry.
  journal.recordChanged("foo/bar"_relpath);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  EXPECT_EQ(1, latest->toSequence);
  EXPECT_EQ(1, latest->fromSequence);
  EXPECT_EQ(nullptr, latest->previous);

  // Add a second entry.
  journal.recordChanged("baz"_relpath);

  // Sanity check that the latest information matches.
  latest = journal.getLatest();
  EXPECT_EQ(2, latest->toSequence);
  EXPECT_EQ(2, latest->fromSequence);
  EXPECT_EQ(1, latest->previous->toSequence);

  // Check basic merge implementation.
  auto merged = latest->merge();
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(2, merged->changedFilesInOverlay.size());
  EXPECT_EQ(nullptr, merged->previous);

  // Let's try with some limits.

  // First just report the most recent item.
  merged = latest->merge(2);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  EXPECT_NE(nullptr, merged->previous);

  // Prune off sequence==1
  merged = latest->merge(2, true);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  EXPECT_EQ(nullptr, merged->previous);

  // Merge the first two entries.
  merged = latest->merge(1);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(2, merged->changedFilesInOverlay.size());
  EXPECT_EQ(nullptr, merged->previous);
}

TEST(Journal, mergeRemoveCreateUpdate) {
  Journal journal;

  // Remove test.txt
  journal.recordRemoved("test.txt"_relpath);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath);
  // Modify test.txt
  journal.recordChanged("test.txt"_relpath);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  EXPECT_EQ(3, latest->toSequence);
  EXPECT_EQ(3, latest->fromSequence);

  // The merged data should report test.txt as changed
  auto merged = latest->merge();
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  ASSERT_EQ(1, merged->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  // Test merging only partway back
  merged = latest->merge(3);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(3, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  ASSERT_EQ(1, merged->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  merged = latest->merge(2);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  ASSERT_EQ(1, merged->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      false,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  merged = latest->merge(1);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  ASSERT_EQ(1, merged->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      merged->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);
}

TEST(Journal, destruction_does_not_overflow_stack_on_long_chain) {
  Journal journal;
  size_t N =
#ifdef NDEBUG
      200000 // Passes in under 200ms.
#else
      40000 // Passes in under 400ms.
#endif
      ;
  for (size_t i = 0; i < N; ++i) {
    journal.recordChanged("foo/bar"_relpath);
  }
}

TEST(Journal, empty_journal_returns_none_for_stats) {
  // Empty journal returns None for stats
  Journal journal;
  auto stats = journal.getStats();
  ASSERT_FALSE(stats.has_value());
}

TEST(Journal, basic_journal_stats) {
  Journal journal;
  // Journal with 1 entry
  journal.recordRemoved("test.txt"_relpath);
  auto from1 = journal.getLatest()->fromTime;
  auto to1 = journal.getLatest()->toTime;
  auto stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(1, stats->entryCount);
  ASSERT_EQ(from1, stats->earliestTimestamp);
  ASSERT_EQ(to1, stats->latestTimestamp);

  // Journal with 2 entries
  journal.recordCreated("test.txt"_relpath);
  stats = journal.getStats();
  auto to2 = journal.getLatest()->toTime;
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(2, stats->entryCount);
  ASSERT_EQ(from1, stats->earliestTimestamp);
  ASSERT_EQ(to2, stats->latestTimestamp);
}

TEST(Journal, memory_usage) {
  Journal journal;
  auto stats = journal.getStats();
  uint64_t prevMem = stats ? stats->memoryUsage : 0;
  for (int i = 0; i < 10; i++) {
    if (i % 2 == 0) {
      journal.recordCreated("test.txt"_relpath);
    } else {
      journal.recordRemoved("test.txt"_relpath);
    }
    stats = journal.getStats();
    uint64_t newMem = stats ? stats->memoryUsage : 0;
    ASSERT_GT(newMem, prevMem);
    prevMem = newMem;
  }
}
