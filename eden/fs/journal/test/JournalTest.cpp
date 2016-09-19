/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/fs/journal/JournalDelta.h"

using namespace facebook::eden;

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
  EXPECT_TRUE(merged != nullptr);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(2, merged->changedFilesInOverlay.size());
  EXPECT_TRUE(merged->previous == nullptr);

  // Let's try with some limits.

  // First just report the most recent item.
  merged = latest->merge(2);
  EXPECT_TRUE(merged != nullptr);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  EXPECT_TRUE(merged->previous != nullptr);

  // Prune off sequence==1
  merged = latest->merge(2, true);
  EXPECT_TRUE(merged != nullptr);
  EXPECT_EQ(2, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(1, merged->changedFilesInOverlay.size());
  EXPECT_TRUE(merged->previous == nullptr);

  // Merge the first two entries.
  merged = latest->merge(1);
  EXPECT_TRUE(merged != nullptr);
  EXPECT_EQ(1, merged->fromSequence);
  EXPECT_EQ(2, merged->toSequence);
  EXPECT_EQ(2, merged->changedFilesInOverlay.size());
  EXPECT_TRUE(merged->previous == nullptr);

  // And replace the journal with our merged result.
  journal.replaceJournal(std::move(merged));
  latest = journal.getLatest();
  EXPECT_EQ(2, latest->toSequence);
  EXPECT_EQ(1, latest->fromSequence);
  EXPECT_TRUE(latest->previous == nullptr);
}
