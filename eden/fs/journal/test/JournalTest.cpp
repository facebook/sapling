/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/journal/Journal.h"
#include <gmock/gmock.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using ::testing::UnorderedElementsAre;

TEST(Journal, accumulate_range_all_changes) {
  Journal journal;

  // Empty journals have no rang to accumulate over
  EXPECT_FALSE(journal.getLatest());
  EXPECT_EQ(nullptr, journal.accumulateRange());

  // Make an initial entry.
  journal.recordChanged("foo/bar"_relpath);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  ASSERT_TRUE(latest);
  EXPECT_EQ(1, latest->sequenceID);

  // Add a second entry.
  journal.recordChanged("baz"_relpath);

  // Sanity check that the latest information matches.
  latest = journal.getLatest();
  ASSERT_TRUE(latest);
  EXPECT_EQ(2, latest->sequenceID);

  // Check basic sum implementation.
  auto summed = journal.accumulateRange();
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(1, summed->fromSequence);
  EXPECT_EQ(2, summed->toSequence);
  EXPECT_EQ(2, summed->changedFilesInOverlay.size());

  // First just report the most recent item.
  summed = journal.accumulateRange(2);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(2, summed->fromSequence);
  EXPECT_EQ(2, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());

  // Merge the first two entries.
  summed = journal.accumulateRange(1);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(1, summed->fromSequence);
  EXPECT_EQ(2, summed->toSequence);
  EXPECT_EQ(2, summed->changedFilesInOverlay.size());
}

TEST(Journal, accumulateRangeRemoveCreateUpdate) {
  Journal journal;

  // Remove test.txt
  journal.recordRemoved("test.txt"_relpath);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath);
  // Modify test.txt
  journal.recordChanged("test.txt"_relpath);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  ASSERT_TRUE(latest);
  EXPECT_EQ(3, latest->sequenceID);

  // The summed data should report test.txt as changed
  auto summed = journal.accumulateRange();
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(1, summed->fromSequence);
  EXPECT_EQ(3, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());
  ASSERT_EQ(1, summed->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  // Test merging only partway back
  summed = journal.accumulateRange(3);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(3, summed->fromSequence);
  EXPECT_EQ(3, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());
  ASSERT_EQ(1, summed->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  summed = journal.accumulateRange(2);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(2, summed->fromSequence);
  EXPECT_EQ(3, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());
  ASSERT_EQ(1, summed->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      false,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);

  summed = journal.accumulateRange(1);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(1, summed->fromSequence);
  EXPECT_EQ(3, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());
  ASSERT_EQ(1, summed->changedFilesInOverlay.count(RelativePath{"test.txt"}));
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedBefore);
  EXPECT_EQ(
      true,
      summed->changedFilesInOverlay[RelativePath{"test.txt"}].existedAfter);
}

TEST(Journal, debugRawJournalInfoRemoveCreateUpdate) {
  Journal journal;

  // Remove test.txt
  journal.recordRemoved("test.txt"_relpath);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath);
  // Modify test.txt
  journal.recordChanged("test.txt"_relpath);

  long mountGen = 333;

  auto debugDeltas = journal.getDebugRawJournalInfo(0, 3, mountGen);
  ASSERT_EQ(3, debugDeltas.size());

  // Debug Raw Journal Info returns info from newest->latest
  EXPECT_TRUE(debugDeltas[0].changedPaths["test.txt"].existedBefore);
  EXPECT_TRUE(debugDeltas[0].changedPaths["test.txt"].existedAfter);
  EXPECT_EQ(debugDeltas[0].fromPosition.mountGeneration, mountGen);
  EXPECT_EQ(debugDeltas[0].fromPosition.sequenceNumber, 3);
  EXPECT_FALSE(debugDeltas[1].changedPaths["test.txt"].existedBefore);
  EXPECT_TRUE(debugDeltas[1].changedPaths["test.txt"].existedAfter);
  EXPECT_EQ(debugDeltas[1].fromPosition.mountGeneration, mountGen);
  EXPECT_EQ(debugDeltas[1].fromPosition.sequenceNumber, 2);
  EXPECT_TRUE(debugDeltas[2].changedPaths["test.txt"].existedBefore);
  EXPECT_FALSE(debugDeltas[2].changedPaths["test.txt"].existedAfter);
  EXPECT_EQ(debugDeltas[2].fromPosition.mountGeneration, mountGen);
  EXPECT_EQ(debugDeltas[2].fromPosition.sequenceNumber, 1);

  debugDeltas = journal.getDebugRawJournalInfo(0, 1, mountGen);
  ASSERT_EQ(1, debugDeltas.size());
  EXPECT_TRUE(debugDeltas[0].changedPaths["test.txt"].existedBefore);
  EXPECT_TRUE(debugDeltas[0].changedPaths["test.txt"].existedAfter);
  EXPECT_EQ(debugDeltas[0].fromPosition.mountGeneration, mountGen);
  EXPECT_EQ(debugDeltas[0].fromPosition.sequenceNumber, 3);

  debugDeltas = journal.getDebugRawJournalInfo(0, 0, mountGen);
  ASSERT_EQ(0, debugDeltas.size());
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
  ASSERT_TRUE(journal.getLatest());
  auto from1 = journal.getLatest()->time;
  auto to1 = journal.getLatest()->time;
  auto stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(1, stats->entryCount);
  ASSERT_EQ(from1, stats->earliestTimestamp);
  ASSERT_EQ(to1, stats->latestTimestamp);

  // Journal with 2 entries
  journal.recordCreated("test.txt"_relpath);
  stats = journal.getStats();
  ASSERT_TRUE(journal.getLatest());
  auto to2 = journal.getLatest()->time;
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

TEST(Journal, set_get_memory_limit) {
  Journal journal;
  journal.setMemoryLimit(500);
  ASSERT_EQ(500, journal.getMemoryLimit());
  journal.setMemoryLimit(333);
  ASSERT_EQ(333, journal.getMemoryLimit());
  journal.setMemoryLimit(0);
  ASSERT_EQ(0, journal.getMemoryLimit());
}

TEST(Journal, limit_of_zero_holds_one_entry) {
  Journal journal;
  // Even though limit is 0, journal will always remember at least one entry
  journal.setMemoryLimit(0);
  // With 1 file we should be able to accumulate from anywhere without
  // truncation, nullptr returned for sequenceID's > 1 (empty ranges)
  journal.recordCreated("file1.txt"_relpath);
  auto summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_FALSE(summed->isTruncated);
  summed = journal.accumulateRange(2);
  EXPECT_FALSE(summed);
}

TEST(Journal, limit_of_zero_truncates_after_one_entry) {
  Journal journal;
  // Even though limit is 0, journal will always remember at least one entry
  journal.setMemoryLimit(0);
  // With 2 files but only one entry in the journal we can only accumulate from
  // sequenceID 2 and above without truncation, nullptr returned for
  // sequenceID's > 2 (empty ranges)
  journal.recordCreated("file1.txt"_relpath);
  journal.recordCreated("file2.txt"_relpath);
  auto summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_TRUE(summed->isTruncated);
  summed = journal.accumulateRange(2);
  ASSERT_TRUE(summed);
  EXPECT_FALSE(summed->isTruncated);
  summed = journal.accumulateRange(3);
  EXPECT_FALSE(summed);
}

TEST(Journal, truncation_nonzero) {
  Journal journal;
  // Set the journal to a size such that it can store a few entries
  journal.setMemoryLimit(1500);
  int totalEntries = 0;
  int rememberedEntries;
  // Keep looping until we get a decent amount of truncation
  do {
    if (totalEntries % 2 == 0) {
      journal.recordCreated("file1.txt"_relpath);
    } else {
      journal.recordRemoved("file1.txt"_relpath);
    }
    ++totalEntries;
    rememberedEntries = journal.getStats()->entryCount;
    auto firstUntruncatedEntry = totalEntries - rememberedEntries + 1;
    for (int j = 1; j < firstUntruncatedEntry; j++) {
      auto summed = journal.accumulateRange(j);
      ASSERT_TRUE(summed);
      // If the value we are accumulating from is more than rememberedEntries
      // from the current sequenceID then it should be truncated
      EXPECT_TRUE(summed->isTruncated)
          << "Failed when remembering " << rememberedEntries
          << " entries out of " << totalEntries
          << " total entries with j = " << j;
    }
    for (int j = firstUntruncatedEntry; j <= totalEntries; j++) {
      auto summed = journal.accumulateRange(j);
      ASSERT_TRUE(summed);
      // If the value we are accumulating from is less than or equal to
      // rememberedEntries from the current sequenceID then it should not be
      // truncated
      EXPECT_FALSE(summed->isTruncated)
          << "Failed when remembering " << rememberedEntries
          << " entries out of " << totalEntries
          << " total entries with j = " << j;
    }
  } while (rememberedEntries + 5 > totalEntries);
}
