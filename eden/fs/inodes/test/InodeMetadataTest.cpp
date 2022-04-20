/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/InodeMetadata.h"

#include <folly/portability/GTest.h>

using namespace facebook::eden;

TEST(InodeMetadataTest, emptyUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata emptyMetadata{};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(emptyMetadata));
}

TEST(InodeMetadataTest, sizeUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      5, std::nullopt, std::nullopt, std::nullopt, std::nullopt, std::nullopt};

  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));
}

TEST(InodeMetadataTest, modeUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt,
      S_IFREG | S_IRWXU,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      std::nullopt,
      S_IFREG | S_IRWXU | S_IRWXG,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt};
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}

TEST(InodeMetadataTest, ownerUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt, std::nullopt, 1, std::nullopt, std::nullopt, std::nullopt};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      std::nullopt, std::nullopt, 3, std::nullopt, std::nullopt, std::nullopt};
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}

TEST(InodeMetadataTest, groupUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt, std::nullopt, std::nullopt, 2, std::nullopt, std::nullopt};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      std::nullopt, std::nullopt, std::nullopt, 4, std::nullopt, std::nullopt};
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}

TEST(InodeMetadataTest, atimeUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      EdenTimestamp{}.toTimespec(),
      std::nullopt};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      timespec{100, 5},
      std::nullopt};
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}

TEST(InodeMetadataTest, mtimeUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      EdenTimestamp{}.toTimespec()};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      std::nullopt,
      timespec{100, 5},
  };
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}

TEST(InodeMetadataTest, mixedUpdate) {
  InodeMetadata basicMetadata{S_IFREG | S_IRWXU, 1, 2, InodeTimestamps{}};
  DesiredMetadata sameMetadata{
      std::nullopt,
      S_IFREG | S_IRWXU,
      1,
      2,
      EdenTimestamp{}.toTimespec(),
      EdenTimestamp{}.toTimespec()};

  EXPECT_TRUE(basicMetadata.shouldShortCircuitMetadataUpdate(sameMetadata));

  DesiredMetadata newMetadata{
      5,
      S_IFREG | S_IRWXU | S_IRWXG,
      3,
      3,
      timespec{100, 5},
      EdenTimestamp{}.toTimespec(),
  };
  EXPECT_FALSE(basicMetadata.shouldShortCircuitMetadataUpdate(newMetadata));
}
#endif
