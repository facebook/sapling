/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include "eden/fs/journal/JournalDelta.h"

using namespace facebook::eden;
using ::testing::UnorderedElementsAre;

TEST(Journal, chain) {
  Journal journal;

  // Make an initial entry.
  auto delta = std::make_unique<JournalDelta>();
  delta->changedFilesInOverlay.insert(RelativePath("foo/bar"));
  journal.addDelta(std::move(delta));

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  EXPECT_EQ(1, latest->toSequence);
  EXPECT_EQ(1, latest->fromSequence);
  EXPECT_EQ(nullptr, latest->previous);

  // Add a second entry.
  delta = std::make_unique<JournalDelta>();
  delta->changedFilesInOverlay.insert(RelativePath("baz"));
  journal.addDelta(std::move(delta));

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

  // And replace the journal with our merged result.
  journal.replaceJournal(std::move(merged));
  latest = journal.getLatest();
  EXPECT_EQ(2, latest->toSequence);
  EXPECT_EQ(1, latest->fromSequence);
  EXPECT_EQ(nullptr, latest->previous);
}

TEST(Journal, mergeRemoveCreateUpdate) {
  Journal journal;

  // Remove test.txt
  auto delta = std::make_unique<JournalDelta>();
  delta->removedFilesInOverlay.insert(RelativePath("test.txt"));
  journal.addDelta(std::move(delta));
  // Create test.txt
  delta = std::make_unique<JournalDelta>();
  delta->createdFilesInOverlay.insert(RelativePath("test.txt"));
  journal.addDelta(std::move(delta));
  // Modify test.txt
  delta = std::make_unique<JournalDelta>();
  delta->changedFilesInOverlay.insert(RelativePath("test.txt"));
  journal.addDelta(std::move(delta));

  // Sanity check that the latest information matches.
  auto latest = journal.getLatest();
  EXPECT_EQ(3, latest->toSequence);
  EXPECT_EQ(3, latest->fromSequence);

  // The merged data should report test.txt as changed
  auto merged = latest->merge();
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_THAT(merged->createdFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(merged->removedFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(
      merged->changedFilesInOverlay,
      UnorderedElementsAre(RelativePath{"test.txt"}));

  // Test merging only partway back
  merged = latest->merge(3);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(3, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_THAT(merged->createdFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(merged->removedFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(
      merged->changedFilesInOverlay,
      UnorderedElementsAre(RelativePath{"test.txt"}));

  merged = latest->merge(2);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_THAT(
      merged->createdFilesInOverlay,
      UnorderedElementsAre(RelativePath{"test.txt"}));
  EXPECT_THAT(merged->removedFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(merged->changedFilesInOverlay, UnorderedElementsAre());

  merged = latest->merge(1);
  ASSERT_NE(nullptr, merged);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(3, merged->toSequence);
  EXPECT_THAT(merged->createdFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(merged->removedFilesInOverlay, UnorderedElementsAre());
  EXPECT_THAT(
      merged->changedFilesInOverlay,
      UnorderedElementsAre(RelativePath{"test.txt"}));
}
