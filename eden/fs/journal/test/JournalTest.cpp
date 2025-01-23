/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/journal/Journal.h"

#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/model/RootId.h"

using namespace facebook::eden;

namespace {

struct IdentityCodec : RootIdCodec {
  RootId parseRootId(folly::StringPiece piece) override {
    return RootId{piece.toString()};
  }
  std::string renderRootId(const RootId& rootId) override {
    return rootId.value();
  }
};

struct JournalTest : ::testing::Test {
  EdenStatsPtr edenStats{makeRefPtr<EdenStats>()};
  Journal journal{edenStats.copy()};
  IdentityCodec codec;
};

struct JournalDeltaTest : ::testing::Test {
  EdenStatsPtr edenStats{makeRefPtr<EdenStats>()};
  Journal journal{edenStats.copy()};
  IdentityCodec codec;

  RootId hash0;
  RootId hash1{"1111111111111111111111111111111111111111"};
  RootId hash2{"2222222222222222222222222222222222222222"};

  std::vector<int> expectedFileChangeSequences;
  std::vector<RelativePathPiece> expectedFileChangeNames;
  std::vector<dtype_t> expectedFileChangeDtypes;
  std::vector<int> expectedHashUpdateSequences;
  std::vector<RootId> expectedHashUpdateHashes;

  std::vector<int> fileChangeSequences;
  std::vector<RelativePathPiece> fileChangeNames;
  std::vector<dtype_t> fileChangeDtypes;
  std::vector<int> hashUpdateSequences;
  std::vector<RootId> hashUpdateHashes;

  void addFileChange(
      RelativePathPiece path,
      dtype_t dtype,
      JournalDelta::SequenceNumber after = 0) {
    journal.recordChanged(path, dtype);
    if (journal.getLatest()->sequenceID >= after) {
      expectedFileChangeNames.push_back(path);
      expectedFileChangeDtypes.push_back(dtype);
      expectedFileChangeSequences.push_back(journal.getLatest()->sequenceID);
    }
  }

  void flush() {
    journal.flush();
    expectedFileChangeDtypes.clear();
    expectedFileChangeNames.clear();
    expectedFileChangeSequences.clear();
    expectedHashUpdateHashes.clear();
    expectedHashUpdateSequences.clear();

    expectedHashUpdateSequences.push_back(journal.getLatest()->sequenceID);
    expectedHashUpdateHashes.push_back(journal.getLatest()->toHash);
  }

  void addHashUpdate(RootId to, JournalDelta::SequenceNumber after = 0) {
    addHashUpdate(hash0, std::move(to), after);
  }

  void addHashUpdate(
      const RootId& from,
      RootId to,
      JournalDelta::SequenceNumber after = 0) {
    journal.recordHashUpdate(from, std::move(to));
    if (journal.getLatest()->sequenceID >= after) {
      expectedHashUpdateSequences.push_back(journal.getLatest()->sequenceID);
      expectedHashUpdateHashes.push_back(from);
    }
  }

  void checkExpect() {
    EXPECT_EQ(expectedFileChangeSequences, fileChangeSequences);
    EXPECT_EQ(expectedFileChangeNames, fileChangeNames);
    EXPECT_EQ(expectedFileChangeDtypes, fileChangeDtypes);
    EXPECT_EQ(expectedHashUpdateSequences, hashUpdateSequences);
    EXPECT_EQ(expectedHashUpdateHashes, hashUpdateHashes);
  }

  /*
    This sets the journal state to be in a post-flush state.
    The current hash will be set to hash1
    The current sequence will be set to 5
  */
  void setupFlushedJournal() {
    journal.recordHashUpdate(hash1);
    journal.recordChanged("foo1"_relpath, dtype_t::Regular);
    journal.recordChanged("foo2"_relpath, dtype_t::Symlink);
    flush();
  }

  /*
   * Set up journal state with a mix of fileChanges and hashUpdates
   */
  void setupGeneric(JournalDelta::SequenceNumber after) {
    addHashUpdate(hash1, after);
    addFileChange("foo1"_relpath, dtype_t::Regular, after);
    addFileChange("foo2"_relpath, dtype_t::Regular, after);
    addFileChange("foo1"_relpath, dtype_t::Regular, after);
    addFileChange("foo2"_relpath, dtype_t::Regular, after);
    EXPECT_EQ(5u, journal.getLatest()->sequenceID);
    addFileChange("foo3"_relpath, dtype_t::Regular, after);
    addFileChange("foo4"_relpath, dtype_t::Regular, after);
    EXPECT_EQ(7u, journal.getLatest()->sequenceID);
    addHashUpdate(hash1, hash2, after);
    addHashUpdate(hash2, hash1, after);
    EXPECT_EQ(9u, journal.getLatest()->sequenceID);
  }
};

} // namespace

TEST_F(JournalTest, accumulate_range_all_changes) {
  // Empty journals have no rang to accumulate over
  EXPECT_FALSE(journal.getLatest());
  EXPECT_EQ(nullptr, journal.accumulateRange());

  // Make an initial entry.
  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  ASSERT_TRUE(latest);
  EXPECT_EQ(1, latest->sequenceID);

  // Add a second entry.
  journal.recordChanged("baz"_relpath, dtype_t::Dir);

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

TEST_F(JournalTest, accumulate_range_mix_hg_changes) {
  // Empty journals have no rang to accumulate over
  EXPECT_FALSE(journal.getLatest());
  EXPECT_EQ(nullptr, journal.accumulateRange());

  // Make an initial entry.
  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();

  // get accumulated data for the tip of journal
  auto summed = journal.accumulateRange(latest->sequenceID);
  EXPECT_FALSE(summed->containsHgOnlyChanges);

  // Record changes under .hg folder
  journal.recordChanged(".hg/foo/bar"_relpath, dtype_t::Dir);

  // get accumulated data for the tip of journal
  latest = journal.getLatest();
  summed = journal.accumulateRange(latest->sequenceID);
  // It only contains .hg change
  EXPECT_TRUE(summed->containsHgOnlyChanges);

  // get accumulated data from the beginning.
  summed = journal.accumulateRange();
  // It contains non-hg-only change
  EXPECT_FALSE(summed->containsHgOnlyChanges);
}

TEST_F(JournalTest, accumulateRangeRemoveCreateUpdate) {
  // Remove test.txt
  journal.recordRemoved("test.txt"_relpath, dtype_t::Regular);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath, dtype_t::Regular);
  // Modify test.txt
  journal.recordChanged("test.txt"_relpath, dtype_t::Regular);

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

namespace {

void checkHashMatches(
    const std::vector<RootId>& transitions,
    Journal& journal) {
  auto latest = journal.getLatest();
  ASSERT_TRUE(latest);
  EXPECT_EQ(transitions.front(), latest->fromHash);
  EXPECT_EQ(transitions.back(), latest->toHash);

  auto range = journal.accumulateRange(latest->sequenceID);
  ASSERT_TRUE(range);
  EXPECT_EQ(transitions, range->snapshotTransitions);
  if (transitions.size() > 1) {
    EXPECT_TRUE(range->containsRootUpdate);
  } else {
    EXPECT_FALSE(range->containsRootUpdate);
  }

  range = journal.accumulateRange();
  ASSERT_TRUE(range);
  EXPECT_EQ(RootId{}, range->snapshotTransitions.front());
  EXPECT_EQ(transitions.back(), range->snapshotTransitions.back());
}

} // namespace

TEST_F(JournalTest, accumulate_range_with_hash_updates) {
  RootId hash0;
  RootId hash1{"1111111111111111111111111111111111111111"};
  RootId hash2{"2222222222222222222222222222222222222222"};
  // Empty journals have no range to accumulate over
  EXPECT_FALSE(journal.getLatest());
  EXPECT_EQ(nullptr, journal.accumulateRange());

  // Make an initial entry.
  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);
  checkHashMatches({hash0}, journal);

  // Update to a new hash using 'to' syntax
  journal.recordHashUpdate(hash1);
  checkHashMatches({hash0, hash1}, journal);

  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);
  checkHashMatches({hash1}, journal);

  // Update to a new hash using 'from/to' syntax
  journal.recordHashUpdate(hash1, hash2);
  checkHashMatches({hash1, hash2}, journal);

  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);
  checkHashMatches({hash2}, journal);

  auto uncleanPaths = std::unordered_set<RelativePath>();
  uncleanPaths.insert(RelativePath("foo/bar"));
  journal.recordUncleanPaths(hash2, hash1, std::move(uncleanPaths));
  checkHashMatches({hash2, hash1}, journal);

  journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);
  checkHashMatches({hash1}, journal);
}

TEST_F(JournalTest, debugRawJournalInfoRemoveCreateUpdate) {
  // Remove test.txt
  journal.recordRemoved("test.txt"_relpath, dtype_t::Regular);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath, dtype_t::Regular);
  // Modify test.txt
  journal.recordChanged("test.txt"_relpath, dtype_t::Regular);

  long mountGen = 333;

  auto debugDeltas = journal.getDebugRawJournalInfo(0, 3, mountGen, codec);
  ASSERT_EQ(3, debugDeltas.size());

  // Debug Raw Journal Info returns info from newest->latest
  EXPECT_TRUE(
      *debugDeltas[0].changedPaths_ref()["test.txt"].existedBefore_ref());
  EXPECT_TRUE(
      *debugDeltas[0].changedPaths_ref()["test.txt"].existedAfter_ref());
  EXPECT_EQ(
      *debugDeltas[0].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[0].fromPosition_ref()->sequenceNumber_ref(), 3);
  EXPECT_FALSE(
      *debugDeltas[1].changedPaths_ref()["test.txt"].existedBefore_ref());
  EXPECT_TRUE(
      *debugDeltas[1].changedPaths_ref()["test.txt"].existedAfter_ref());
  EXPECT_EQ(
      *debugDeltas[1].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[1].fromPosition_ref()->sequenceNumber_ref(), 2);
  EXPECT_TRUE(
      *debugDeltas[2].changedPaths_ref()["test.txt"].existedBefore_ref());
  EXPECT_FALSE(
      *debugDeltas[2].changedPaths_ref()["test.txt"].existedAfter_ref());
  EXPECT_EQ(
      *debugDeltas[2].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[2].fromPosition_ref()->sequenceNumber_ref(), 1);

  debugDeltas = journal.getDebugRawJournalInfo(0, 1, mountGen, codec);
  ASSERT_EQ(1, debugDeltas.size());
  EXPECT_TRUE(
      *debugDeltas[0].changedPaths_ref()["test.txt"].existedBefore_ref());
  EXPECT_TRUE(
      *debugDeltas[0].changedPaths_ref()["test.txt"].existedAfter_ref());
  EXPECT_EQ(
      *debugDeltas[0].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[0].fromPosition_ref()->sequenceNumber_ref(), 3);

  debugDeltas = journal.getDebugRawJournalInfo(0, 0, mountGen, codec);
  ASSERT_EQ(0, debugDeltas.size());
}

TEST_F(JournalTest, debugRawJournalInfoHashUpdates) {
  auto hash0 = RootId{};
  auto hash1 = RootId{"1111111111111111111111111111111111111111"};
  auto hash2 = RootId{"2222222222222222222222222222222222222222"};

  // Go from hash0 to hash1
  journal.recordHashUpdate(hash0, hash1);
  // Create test.txt
  journal.recordCreated("test.txt"_relpath, dtype_t::Regular);
  // Go from hash1 to hash2
  journal.recordHashUpdate(hash1, hash2);

  long mountGen = 333;

  auto debugDeltas = journal.getDebugRawJournalInfo(0, 3, mountGen, codec);
  ASSERT_EQ(3, debugDeltas.size());

  // Debug Raw Journal Info returns info from newest->latest
  EXPECT_TRUE(debugDeltas[0].changedPaths_ref()->empty());
  EXPECT_EQ(
      *debugDeltas[0].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[0].fromPosition_ref()->sequenceNumber_ref(), 3);
  EXPECT_EQ(
      *debugDeltas[0].fromPosition_ref()->snapshotHash_ref(), hash1.value());
  EXPECT_EQ(
      *debugDeltas[0].toPosition_ref()->snapshotHash_ref(), hash2.value());
  EXPECT_FALSE(
      *debugDeltas[1].changedPaths_ref()["test.txt"].existedBefore_ref());
  EXPECT_TRUE(
      *debugDeltas[1].changedPaths_ref()["test.txt"].existedAfter_ref());
  EXPECT_EQ(
      *debugDeltas[1].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[1].fromPosition_ref()->sequenceNumber_ref(), 2);
  EXPECT_EQ(
      *debugDeltas[1].fromPosition_ref()->snapshotHash_ref(), hash1.value());
  EXPECT_EQ(
      *debugDeltas[1].toPosition_ref()->snapshotHash_ref(), hash1.value());
  EXPECT_TRUE(debugDeltas[2].changedPaths_ref()->empty());
  EXPECT_EQ(
      *debugDeltas[2].fromPosition_ref()->mountGeneration_ref(), mountGen);
  EXPECT_EQ(*debugDeltas[2].fromPosition_ref()->sequenceNumber_ref(), 1);
  EXPECT_EQ(
      *debugDeltas[2].fromPosition_ref()->snapshotHash_ref(), hash0.value());
  EXPECT_EQ(
      *debugDeltas[2].toPosition_ref()->snapshotHash_ref(), hash1.value());
}

TEST_F(JournalTest, destruction_does_not_overflow_stack_on_long_chain) {
  size_t N =
#ifdef NDEBUG
      200000 // Passes in under 200ms.
#else
      40000 // Passes in under 400ms.
#endif
      ;
  for (size_t i = 0; i < N; ++i) {
    journal.recordChanged("foo/bar"_relpath, dtype_t::Dir);
  }
}

TEST_F(JournalTest, empty_journal_returns_none_for_stats) {
  auto stats = journal.getStats();
  ASSERT_FALSE(stats.has_value());
}

TEST_F(JournalTest, basic_journal_stats) {
  // Journal with 1 entry
  journal.recordRemoved("test.txt"_relpath, dtype_t::Regular);
  ASSERT_TRUE(journal.getLatest());
  auto from1 = journal.getLatest()->time;
  auto to1 = journal.getLatest()->time;
  auto stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(1, stats->entryCount);
  ASSERT_EQ(from1, stats->earliestTimestamp);
  ASSERT_EQ(to1, stats->latestTimestamp);

  // Journal with 2 entries
  journal.recordCreated("test.txt"_relpath, dtype_t::Regular);
  stats = journal.getStats();
  ASSERT_TRUE(journal.getLatest());
  auto to2 = journal.getLatest()->time;
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(2, stats->entryCount);
  ASSERT_EQ(from1, stats->earliestTimestamp);
  ASSERT_EQ(to2, stats->latestTimestamp);
}

TEST_F(JournalTest, truncated_read_stats) {
  journal.setMemoryLimit(0);
  journal.recordCreated("test1.txt"_relpath, dtype_t::Regular);
  journal.recordRemoved("test1.txt"_relpath, dtype_t::Regular);

  auto data = facebook::fb303::ServiceData::get();
  constexpr folly::StringPiece key = "journal.truncated_reads.sum";
  edenStats->flush();
  auto initialValue = data->getCounter(key);

  // Empty Accumulate range, should be 0 files accumulated
  journal.accumulateRange(3);
  edenStats->flush();
  ASSERT_EQ(0, data->getCounter(key) - initialValue);

  // This is not a truncated read since journal remembers at least one entry
  journal.accumulateRange(2);
  edenStats->flush();
  ASSERT_EQ(0, data->getCounter(key) - initialValue);

  journal.accumulateRange(1);
  edenStats->flush();
  ASSERT_EQ(1, data->getCounter(key) - initialValue);

  journal.accumulateRange(2);
  edenStats->flush();
  ASSERT_EQ(1, data->getCounter(key) - initialValue);

  journal.accumulateRange(1);
  edenStats->flush();
  ASSERT_EQ(2, data->getCounter(key) - initialValue);
}

TEST_F(JournalTest, files_accumulated_stats) {
  journal.recordCreated("test1.txt"_relpath, dtype_t::Regular);
  journal.recordRemoved("test1.txt"_relpath, dtype_t::Regular);

  auto data = facebook::fb303::ServiceData::get();
  constexpr folly::StringPiece key = "journal.files_accumulated.sum";
  edenStats->flush();
  auto initialValue = data->getCounter(key);
  ASSERT_EQ(0, journal.getStats()->maxFilesAccumulated);

  // Empty Accumulate range, should be 0 files accumulated
  journal.accumulateRange(3);
  edenStats->flush();
  ASSERT_EQ(0, data->getCounter(key) - initialValue);
  ASSERT_EQ(0, journal.getStats()->maxFilesAccumulated);

  journal.accumulateRange(2);
  edenStats->flush();
  ASSERT_EQ(1, data->getCounter(key) - initialValue);
  ASSERT_EQ(1, journal.getStats()->maxFilesAccumulated);

  journal.accumulateRange(1);
  edenStats->flush();
  ASSERT_EQ(3, data->getCounter(key) - initialValue);
  ASSERT_EQ(2, journal.getStats()->maxFilesAccumulated);

  journal.accumulateRange(2);
  edenStats->flush();
  ASSERT_EQ(4, data->getCounter(key) - initialValue);
  ASSERT_EQ(2, journal.getStats()->maxFilesAccumulated);
}

TEST_F(JournalTest, memory_usage) {
  auto stats = journal.getStats();
  uint64_t prevMem = journal.estimateMemoryUsage();
  for (int i = 0; i < 10; i++) {
    if (i % 2 == 0) {
      journal.recordCreated("test.txt"_relpath, dtype_t::Regular);
    } else {
      journal.recordRemoved("test.txt"_relpath, dtype_t::Regular);
    }
    stats = journal.getStats();
    uint64_t newMem = journal.estimateMemoryUsage();
    ASSERT_GT(newMem, prevMem);
    prevMem = newMem;
  }
}

TEST_F(JournalTest, set_get_memory_limit) {
  journal.setMemoryLimit(500);
  ASSERT_EQ(500, journal.getMemoryLimit());
  journal.setMemoryLimit(333);
  ASSERT_EQ(333, journal.getMemoryLimit());
  journal.setMemoryLimit(0);
  ASSERT_EQ(0, journal.getMemoryLimit());
}

TEST_F(JournalTest, truncation_by_flush) {
  journal.recordCreated("file1.txt"_relpath, dtype_t::Regular);
  journal.recordCreated("file2.txt"_relpath, dtype_t::Regular);
  journal.recordCreated("file3.txt"_relpath, dtype_t::Regular);
  auto summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_FALSE(summed->isTruncated);
  journal.flush();
  summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_TRUE(summed->isTruncated);
}

TEST_F(JournalTest, limit_of_zero_holds_one_entry) {
  // Even though limit is 0, journal will always remember at least one entry
  journal.setMemoryLimit(0);
  // With 1 file we should be able to accumulate from anywhere without
  // truncation, nullptr returned for sequenceID's > 1 (empty ranges)
  journal.recordCreated("file1.txt"_relpath, dtype_t::Regular);
  auto summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_FALSE(summed->isTruncated);
  summed = journal.accumulateRange(2);
  EXPECT_FALSE(summed);
}

TEST_F(JournalTest, limit_of_zero_truncates_after_one_entry) {
  // Even though limit is 0, journal will always remember at least one entry
  journal.setMemoryLimit(0);
  // With 2 files but only one entry in the journal we can only accumulate from
  // sequenceID 2 and above without truncation, nullptr returned for
  // sequenceID's > 2 (empty ranges)
  journal.recordCreated("file1.txt"_relpath, dtype_t::Regular);
  journal.recordCreated("file2.txt"_relpath, dtype_t::Regular);
  auto summed = journal.accumulateRange(1);
  ASSERT_TRUE(summed);
  EXPECT_TRUE(summed->isTruncated);
  summed = journal.accumulateRange(2);
  ASSERT_TRUE(summed);
  EXPECT_FALSE(summed->isTruncated);
  summed = journal.accumulateRange(3);
  EXPECT_FALSE(summed);
}

TEST_F(JournalTest, truncation_nonzero) {
  // Set the journal to a size such that it can store a few entries
  journal.setMemoryLimit(1500);
  int totalEntries = 0;
  int rememberedEntries;
  // Keep looping until we get a decent amount of truncation
  do {
    if (totalEntries % 2 == 0) {
      journal.recordCreated("file1.txt"_relpath, dtype_t::Regular);
    } else {
      journal.recordRemoved("file1.txt"_relpath, dtype_t::Regular);
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

TEST_F(JournalTest, compaction) {
  journal.recordCreated("file1.txt"_relpath, dtype_t::Regular);
  auto stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(1, stats->entryCount);
  auto latest = journal.getLatest();
  ASSERT_TRUE(latest);
  ASSERT_EQ(1, latest->sequenceID);

  journal.recordChanged("file1.txt"_relpath, dtype_t::Regular);
  stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(2, stats->entryCount);
  latest = journal.getLatest();
  ASSERT_TRUE(latest);
  ASSERT_EQ(2, latest->sequenceID);
  auto summed = journal.accumulateRange(2);
  ASSERT_NE(nullptr, summed);
  EXPECT_EQ(2, summed->fromSequence);
  EXPECT_EQ(2, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());

  // Changing file1.txt again should just change the sequenceID of the last
  // delta to be 3
  journal.recordChanged("file1.txt"_relpath, dtype_t::Regular);
  stats = journal.getStats();
  ASSERT_TRUE(stats.has_value());
  ASSERT_EQ(2, stats->entryCount);
  latest = journal.getLatest();
  ASSERT_TRUE(latest);
  ASSERT_EQ(3, latest->sequenceID);
  summed = journal.accumulateRange(2);
  ASSERT_NE(nullptr, summed);
  // We expect from to be 3 since there is no delta with sequence ID = 2
  EXPECT_EQ(3, summed->fromSequence);
  EXPECT_EQ(3, summed->toSequence);
  EXPECT_EQ(1, summed->changedFilesInOverlay.size());
}

TEST_F(JournalTest, update_transitions_are_all_recorded) {
  RootId hash1{"0000000000000000000000000000000000000001"};
  RootId hash2{"0000000000000000000000000000000000000002"};
  RootId hash3{"0000000000000000000000000000000000000003"};
  journal.recordHashUpdate(hash1, hash2);
  journal.recordHashUpdate(hash2, hash3);

  auto summed = journal.accumulateRange();
  EXPECT_EQ(3, summed->snapshotTransitions.size());
  EXPECT_EQ(hash1, summed->snapshotTransitions[0]);
  EXPECT_EQ(hash2, summed->snapshotTransitions[1]);
  EXPECT_EQ(hash3, summed->snapshotTransitions[2]);
}

TEST_F(JournalTest, update_transitions_are_coalesced) {
  RootId hash1{"0000000000000000000000000000000000000001"};
  RootId hash2{"0000000000000000000000000000000000000002"};
  RootId hash3{"0000000000000000000000000000000000000003"};
  journal.recordHashUpdate(hash1, hash2);
  journal.recordHashUpdate(hash2, hash2);
  journal.recordHashUpdate(hash2, hash3);

  auto summed = journal.accumulateRange();
  EXPECT_EQ(3, summed->snapshotTransitions.size());
  EXPECT_EQ(hash1, summed->snapshotTransitions[0]);
  EXPECT_EQ(hash2, summed->snapshotTransitions[1]);
  EXPECT_EQ(hash3, summed->snapshotTransitions[2]);
}

TEST_F(JournalTest, update_transitions_with_unclean_files_are_not_coalesced) {
  RootId hash1{"0000000000000000000000000000000000000001"};
  RootId hash2{"0000000000000000000000000000000000000002"};
  RootId hash3{"0000000000000000000000000000000000000003"};
  journal.recordHashUpdate(hash1, hash2);
  journal.recordUncleanPaths(hash2, hash2, {RelativePath{"foo"}});
  journal.recordHashUpdate(hash2, hash3);

  auto summed = journal.accumulateRange();
  EXPECT_EQ(4, summed->snapshotTransitions.size());
  EXPECT_EQ(hash1, summed->snapshotTransitions[0]);
  EXPECT_EQ(hash2, summed->snapshotTransitions[1]);
  EXPECT_EQ(hash2, summed->snapshotTransitions[2]);
  EXPECT_EQ(hash3, summed->snapshotTransitions[3]);
}

TEST_F(JournalTest, subscribers_are_notified_of_changes) {
  unsigned calls = 0;
  auto sub = journal.registerSubscriber([&] { ++calls; });
  (void)sub;

  EXPECT_EQ(0u, calls);
  journal.recordChanged("foo"_relpath, dtype_t::Dir);
  EXPECT_EQ(1u, calls);
  EXPECT_EQ(1u, journal.getLatest()->sequenceID);

  journal.recordChanged("foo"_relpath, dtype_t::Dir);
  EXPECT_EQ(2u, calls);
  EXPECT_EQ(2u, journal.getLatest()->sequenceID);
}

TEST_F(
    JournalTest,
    subscribers_are_not_notified_of_changes_until_they_are_observed) {
  unsigned calls = 0;
  auto sub = journal.registerSubscriber([&] { ++calls; });
  (void)sub;

  EXPECT_EQ(0u, calls);
  journal.recordChanged("foo"_relpath, dtype_t::Regular);
  EXPECT_EQ(1u, calls);
  journal.recordChanged("foo"_relpath, dtype_t::Regular);
  EXPECT_EQ(1u, calls);
  EXPECT_EQ(2u, journal.getLatest()->sequenceID);
  journal.recordChanged("foo"_relpath, dtype_t::Regular);
  EXPECT_EQ(2u, calls);
  EXPECT_EQ(3u, journal.getLatest()->sequenceID);
}

TEST_F(JournalTest, all_subscribers_are_notified_after_any_observation) {
  unsigned calls1 = 0;
  unsigned calls2 = 0;
  auto sub1 = journal.registerSubscriber([&] { ++calls1; });
  (void)sub1;
  auto sub2 = journal.registerSubscriber([&] { ++calls2; });
  (void)sub2;

  EXPECT_EQ(0u, calls1);
  EXPECT_EQ(0u, calls2);

  journal.recordChanged("foo"_relpath, dtype_t::Regular);
  journal.recordChanged("foo"_relpath, dtype_t::Regular);

  EXPECT_EQ(1u, calls1);
  EXPECT_EQ(1u, calls2);

  EXPECT_EQ(2u, journal.getLatest()->sequenceID);
  journal.recordChanged("foo"_relpath, dtype_t::Regular);

  EXPECT_EQ(2u, calls1);
  EXPECT_EQ(2u, calls2);

  journal.recordChanged("foo"_relpath, dtype_t::Regular);

  EXPECT_EQ(2u, calls1);
  EXPECT_EQ(2u, calls2);
}

TEST_F(JournalTest, for_each_delta_forwards) {
  RootId hash1{"1111111111111111111111111111111111111111"};
  RootId hash2{"2222222222222222222222222222222222222222"};
  std::vector<int> expectedFileChangeSequences{1, 2, 3, 4, 6, 7};
  std::vector<int> expectedHashUpdateSequences{5, 8};
  std::vector<RelativePathPiece> expectedFileChangeNames{
      "foo1"_relpath,
      "foo2"_relpath,
      "foo3"_relpath,
      "foo4"_relpath,
      "foo6"_relpath,
      "foo7"_relpath};
  std::vector<dtype_t> expectedFileChangeDtypes{
      dtype_t::Regular,
      dtype_t::Symlink,
      dtype_t::Regular,
      dtype_t::Symlink,
      dtype_t::Regular,
      dtype_t::Regular};
  std::vector<RootId> expectedHashUpdateHashes{hash1, hash2};

  std::vector<int> fileChangeSequences;
  std::vector<int> hashUpdateSequences;
  std::vector<RelativePathPiece> fileChangeNames;
  std::vector<RootId> hashUpdateHashes;
  std::vector<dtype_t> fileChangeDtypes;

  journal.recordChanged("foo1"_relpath, dtype_t::Regular);
  journal.recordChanged("foo2"_relpath, dtype_t::Symlink);
  EXPECT_EQ(2u, journal.getLatest()->sequenceID);
  journal.recordChanged("foo3"_relpath, dtype_t::Regular);
  journal.recordChanged("foo4"_relpath, dtype_t::Symlink);
  EXPECT_EQ(4u, journal.getLatest()->sequenceID);
  journal.recordHashUpdate(hash1, hash2);
  EXPECT_EQ(5u, journal.getLatest()->sequenceID);
  journal.recordChanged("foo6"_relpath, dtype_t::Regular);
  journal.recordChanged("foo7"_relpath, dtype_t::Regular);
  journal.recordHashUpdate(hash2, hash1);

  bool truncated = journal.forEachDeltaForwards(
      1u,
      [&](const FileChangeJournalDelta& current) -> bool {
        fileChangeSequences.push_back(current.sequenceID);
        fileChangeNames.push_back(current.path1);
        fileChangeDtypes.push_back(current.type);
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        hashUpdateSequences.push_back(current.sequenceID);
        hashUpdateHashes.push_back(current.fromHash);
        return true;
      });
  EXPECT_FALSE(truncated);
  EXPECT_EQ(expectedFileChangeSequences, fileChangeSequences);
  EXPECT_EQ(expectedFileChangeNames, fileChangeNames);
  EXPECT_EQ(expectedFileChangeDtypes, fileChangeDtypes);
  EXPECT_EQ(expectedHashUpdateHashes, hashUpdateHashes);
}

/*
 * This test covers the case where 'from' is a value below the sequence number
 * of the first delta in fileChanges and there are hashUpdates present between
 * the two. It checks that fileChanges starts from the first entry in the
 * fileChanges vector.
 */
TEST_F(JournalDeltaTest, for_each_delta_forwards_file_change_ends_above_from) {
  setupFlushedJournal();
  EXPECT_EQ(5u, journal.getLatest()->sequenceID);

  // Create hashUpdates after from and before file changes
  addHashUpdate(hash1, hash2);
  addHashUpdate(hash2, hash1);
  EXPECT_EQ(7u, journal.getLatest()->sequenceID);

  // Create file changes
  addFileChange("foo3"_relpath, dtype_t::Regular);
  addFileChange("foo4"_relpath, dtype_t::Symlink);
  EXPECT_EQ(9u, journal.getLatest()->sequenceID);

  bool truncated = journal.forEachDeltaForwards(
      5u,
      [&](const FileChangeJournalDelta& current) -> bool {
        fileChangeSequences.push_back(current.sequenceID);
        fileChangeNames.push_back(current.path1);
        fileChangeDtypes.push_back(current.type);
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        hashUpdateSequences.push_back(current.sequenceID);
        hashUpdateHashes.push_back(current.fromHash);
        return true;
      });
  EXPECT_FALSE(truncated);
  checkExpect();
}

/*
 * This test covers the case where 'from' is a value below the sequence number
 * of the first delta in hashUpdates and there are fileChanges present between
 * the two. It checks that hashUpdates starts from the first entry in the
 * hashUpdates vector.
 */
TEST_F(JournalDeltaTest, for_each_delta_forwards_hash_update_ends_above_from) {
  setupFlushedJournal();
  EXPECT_EQ(5u, journal.getLatest()->sequenceID);
  // Create file changes after from and before hashUpdates
  addFileChange("foo3"_relpath, dtype_t::Regular);
  addFileChange("foo4"_relpath, dtype_t::Symlink);
  EXPECT_EQ(7u, journal.getLatest()->sequenceID);
  // Create hashUpdates
  addHashUpdate(hash1, hash2);
  addHashUpdate(hash2, hash1);
  EXPECT_EQ(9u, journal.getLatest()->sequenceID);

  bool truncated = journal.forEachDeltaForwards(
      5u,
      [&](const FileChangeJournalDelta& current) -> bool {
        fileChangeSequences.push_back(current.sequenceID);
        fileChangeNames.push_back(current.path1);
        fileChangeDtypes.push_back(current.type);
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        hashUpdateSequences.push_back(current.sequenceID);
        hashUpdateHashes.push_back(current.fromHash);
        return true;
      });
  EXPECT_FALSE(truncated);
  checkExpect();
}

/*
 * Tests that when 'from' is in the middle of the result set,
 * returns all results starting from that value
 */
TEST_F(JournalDeltaTest, for_each_delta_forwards_partial_results) {
  setupGeneric(6u);
  bool truncated = journal.forEachDeltaForwards(
      6u,
      [&](const FileChangeJournalDelta& current) -> bool {
        fileChangeSequences.push_back(current.sequenceID);
        fileChangeNames.push_back(current.path1);
        fileChangeDtypes.push_back(current.type);
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        hashUpdateSequences.push_back(current.sequenceID);
        hashUpdateHashes.push_back(current.fromHash);
        return true;
      });
  EXPECT_FALSE(truncated);
  checkExpect();
}

/*
 * Tests that when 'from' is higher than the current sequence id,
 * returns no values.
 */
TEST_F(JournalDeltaTest, for_each_delta_forwards_no_results) {
  setupGeneric(10u);
  bool truncated = journal.forEachDeltaForwards(
      10u,
      [&](const FileChangeJournalDelta& current) -> bool {
        fileChangeSequences.push_back(current.sequenceID);
        fileChangeNames.push_back(current.path1);
        fileChangeDtypes.push_back(current.type);
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        hashUpdateSequences.push_back(current.sequenceID);
        hashUpdateHashes.push_back(current.fromHash);
        return true;
      });
  EXPECT_FALSE(truncated);
  checkExpect();
}
