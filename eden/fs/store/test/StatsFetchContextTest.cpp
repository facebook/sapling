/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/StatsFetchContext.h"
#include <gtest/gtest.h>
#include "eden/fs/model/ObjectId.h"

namespace facebook::eden {

namespace {

ObjectId makeTestId(const char* hex) {
  return ObjectId::fromHex(hex);
}

} // namespace

TEST(StatsFetchContextTest, DidFetchTracksBytes) {
  StatsFetchContext ctx;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");

  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 1024);
  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 2048);

  EXPECT_EQ(
      2,
      ctx.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
  EXPECT_EQ(
      3072,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
}

TEST(StatsFetchContextTest, DidFetchBatchAggregates) {
  StatsFetchContext ctx;

  ctx.didFetchBatch(
      ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache, 100, 50000);

  EXPECT_EQ(
      100,
      ctx.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache));
  EXPECT_EQ(
      50000,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache));
}

TEST(StatsFetchContextTest, ComputeStatisticsIncludesBytes) {
  StatsFetchContext ctx;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");

  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromMemoryCache, 100);
  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromDiskCache, 200);
  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 300);

  auto stats = ctx.computeStatistics();
  EXPECT_EQ(600, stats.blob.totalBytes);
  EXPECT_EQ(3, stats.blob.accessCount);
}

TEST(StatsFetchContextTest, CopyConstructorPreservesBytes) {
  StatsFetchContext ctx1;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");
  ctx1.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 1000);

  StatsFetchContext ctx2(ctx1);

  EXPECT_EQ(
      1,
      ctx2.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
  EXPECT_EQ(
      1000,
      ctx2.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
}

TEST(StatsFetchContextTest, MergeAddsBytes) {
  StatsFetchContext ctx1, ctx2;
  ObjectId id1 = makeTestId("1111111111111111111111111111111111111111");
  ObjectId id2 = makeTestId("2222222222222222222222222222222222222222");

  ctx1.didFetch(
      ObjectFetchContext::Tree, id1, ObjectFetchContext::FromDiskCache, 500);
  ctx2.didFetch(
      ObjectFetchContext::Tree, id2, ObjectFetchContext::FromDiskCache, 700);

  ctx1.merge(ctx2);

  EXPECT_EQ(
      2,
      ctx1.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache));
  EXPECT_EQ(
      1200,
      ctx1.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache));
}

TEST(StatsFetchContextTest, DidFetchWithoutBytesStillWorks) {
  StatsFetchContext ctx;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");

  // Use the old API without bytes parameter
  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch);

  EXPECT_EQ(1, ctx.countFetchesOfType(ObjectFetchContext::Blob));
  // Bytes should be 0 when using the old API
  EXPECT_EQ(
      0,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
}

TEST(StatsFetchContextTest, DidFetchFailedTracksFailures) {
  StatsFetchContext ctx;

  ctx.didFetchFailed(ObjectFetchContext::Blob, 5);
  ctx.didFetchFailed(ObjectFetchContext::Blob, 3);

  EXPECT_EQ(8, ctx.getFailureCount(ObjectFetchContext::Blob));
  EXPECT_EQ(0, ctx.getFailureCount(ObjectFetchContext::Tree));
}

TEST(StatsFetchContextTest, CopyConstructorPreservesFailures) {
  StatsFetchContext ctx1;
  ctx1.didFetchFailed(ObjectFetchContext::Blob, 10);

  StatsFetchContext ctx2(ctx1);

  EXPECT_EQ(10, ctx2.getFailureCount(ObjectFetchContext::Blob));
}

TEST(StatsFetchContextTest, MergeAddsFailures) {
  StatsFetchContext ctx1, ctx2;

  ctx1.didFetchFailed(ObjectFetchContext::Blob, 5);
  ctx2.didFetchFailed(ObjectFetchContext::Blob, 7);

  ctx1.merge(ctx2);

  EXPECT_EQ(12, ctx1.getFailureCount(ObjectFetchContext::Blob));
}

TEST(StatsFetchContextTest, MoveConstructorPreservesBytes) {
  StatsFetchContext ctx1;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");
  ctx1.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 1000);
  ctx1.didFetchFailed(ObjectFetchContext::Blob, 3);

  StatsFetchContext ctx2(std::move(ctx1));

  EXPECT_EQ(
      1,
      ctx2.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
  EXPECT_EQ(
      1000,
      ctx2.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
  EXPECT_EQ(3, ctx2.getFailureCount(ObjectFetchContext::Blob));
}

TEST(StatsFetchContextTest, MoveAssignmentPreservesBytes) {
  StatsFetchContext ctx1;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");
  ctx1.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromDiskCache, 2000);
  ctx1.didFetchFailed(ObjectFetchContext::Tree, 7);

  StatsFetchContext ctx2;
  ctx2 = std::move(ctx1);

  EXPECT_EQ(
      1,
      ctx2.countFetchesOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromDiskCache));
  EXPECT_EQ(
      2000,
      ctx2.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromDiskCache));
  EXPECT_EQ(7, ctx2.getFailureCount(ObjectFetchContext::Tree));
}

TEST(StatsFetchContextTest, BytesTrackedPerObjectType) {
  StatsFetchContext ctx;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");

  ctx.didFetch(
      ObjectFetchContext::Blob, id, ObjectFetchContext::FromNetworkFetch, 100);
  ctx.didFetch(
      ObjectFetchContext::Tree, id, ObjectFetchContext::FromDiskCache, 200);

  EXPECT_EQ(
      100,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromNetworkFetch));
  EXPECT_EQ(
      200,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromDiskCache));
  // No cross-contamination between types/origins
  EXPECT_EQ(
      0,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Blob, ObjectFetchContext::FromDiskCache));
  EXPECT_EQ(
      0,
      ctx.countBytesFetchedOfTypeAndOrigin(
          ObjectFetchContext::Tree, ObjectFetchContext::FromNetworkFetch));
}

TEST(StatsFetchContextTest, ComputeStatisticsForAllTypes) {
  StatsFetchContext ctx;
  ObjectId id = makeTestId("1234567890123456789012345678901234567890");

  ctx.didFetch(
      ObjectFetchContext::Tree, id, ObjectFetchContext::FromNetworkFetch, 400);
  ctx.didFetch(
      ObjectFetchContext::Tree, id, ObjectFetchContext::FromMemoryCache, 100);
  ctx.didFetch(
      ObjectFetchContext::BlobAuxData,
      id,
      ObjectFetchContext::FromDiskCache,
      50);

  auto stats = ctx.computeStatistics();

  EXPECT_EQ(500, stats.tree.totalBytes);
  EXPECT_EQ(2, stats.tree.accessCount);
  EXPECT_EQ(1, stats.tree.fetchCount);

  EXPECT_EQ(50, stats.blobAuxData.totalBytes);
  EXPECT_EQ(1, stats.blobAuxData.accessCount);

  EXPECT_EQ(0, stats.blob.totalBytes);
  EXPECT_EQ(0, stats.blob.accessCount);
}

} // namespace facebook::eden
