/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FilterUtils.h"
#include <folly/portability/GTest.h>
#include "eden/fs/store/FilteredBackingStore.h"

using namespace facebook::eden;

TEST(FilterUtilsTest, filterContainsNullByte) {
  auto filterWithNullByte = std::string{"foo\0bar", 7};
  auto startingRootId = "OriginalRoot";
  auto filteredRootId = FilteredBackingStore::createFilteredRootId(
      startingRootId, filterWithNullByte);
  auto [rootId, filter] = parseFilterIdFromRootId(RootId{filteredRootId});
  EXPECT_EQ(startingRootId, rootId.value());
  EXPECT_EQ(filter, filterWithNullByte);
}

TEST(FilterUtilsTest, basic) {
  auto startingFilter = "foobar";
  auto startingRootId = "OriginalRoot";
  auto filteredRootId = FilteredBackingStore::createFilteredRootId(
      startingRootId, startingFilter);
  auto [rootId, filterFromRootId] =
      parseFilterIdFromRootId(RootId{filteredRootId});
  EXPECT_EQ(startingFilter, filterFromRootId);
  EXPECT_EQ(startingRootId, rootId.value());
}
