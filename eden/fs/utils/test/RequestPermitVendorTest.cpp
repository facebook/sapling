/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/RequestPermitVendor.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(RequestPermitVendorTest, AcquirePermitSimple) {
  auto vendor = RequestPermitVendor(1);
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);

  auto p1 = vendor.acquirePermit();
  EXPECT_NE(p1, nullptr);
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 0);
  EXPECT_EQ(vendor.inflight(), 1);

  p1.reset();
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);
}

TEST(RequestPermitVendorTest, AcquirePermitSimpleScopeDestruction) {
  auto vendor = RequestPermitVendor(1);
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);

  {
    auto p1 = vendor.acquirePermit();
    EXPECT_NE(p1, nullptr);
    EXPECT_EQ(vendor.capacity(), 1);
    EXPECT_EQ(vendor.available(), 0);
    EXPECT_EQ(vendor.inflight(), 1);
  }

  // p1 is out of scope, the capacity should be released back to the vendor
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);
}

TEST(RequestPermitVendorTest, EnsureAcquirePermitOverCapacityBlocks) {
  auto vendor = RequestPermitVendor(1);
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);

  auto p1 = vendor.acquirePermit();
  EXPECT_NE(p1, nullptr);
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 0);
  EXPECT_EQ(vendor.inflight(), 1);

  folly::Future<std::unique_ptr<RequestPermit>> future =
      folly::makeFuture()
          .via(folly::getGlobalCPUExecutor().get())
          .thenValue([&](auto&&) { return vendor.acquirePermit(); });

  EXPECT_EQ(future.isReady(), false);

  // Destroy the first RequestPermit, which should unblock the acquirePermit()
  // in the future
  p1.reset();

  std::unique_ptr<RequestPermit> p2;
  try {
    p2 = std::move(future).get(std::chrono::milliseconds(100));
  } catch (const folly::FutureInvalid&) {
    FAIL() << "Future was not ready when it was expected to be";
  }

  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 0);
  EXPECT_EQ(vendor.inflight(), 1);

  p2.reset();
  EXPECT_EQ(vendor.capacity(), 1);
  EXPECT_EQ(vendor.available(), 1);
  EXPECT_EQ(vendor.inflight(), 0);
}

TEST(RequestPermitVendorTest, AcquirePermitLargeLimit) {
  auto vendor = RequestPermitVendor(100);
  EXPECT_EQ(vendor.capacity(), 100);
  EXPECT_EQ(vendor.available(), 100);
  EXPECT_EQ(vendor.inflight(), 0);

  auto permits = std::vector<std::unique_ptr<RequestPermit>>{};

  for (int i = 0; i < 100; ++i) {
    permits.push_back(vendor.acquirePermit());
  }
  EXPECT_EQ(permits.size(), 100);
  EXPECT_EQ(vendor.capacity(), 100);
  EXPECT_EQ(vendor.available(), 0);
  EXPECT_EQ(vendor.inflight(), 100);

  permits.clear();
  EXPECT_EQ(permits.size(), 0);
  EXPECT_EQ(vendor.capacity(), 100);
  EXPECT_EQ(vendor.available(), 100);
  EXPECT_EQ(vendor.inflight(), 0);

  for (int i = 0; i < 50; ++i) {
    permits.push_back(vendor.acquirePermit());
  }
  EXPECT_EQ(permits.size(), 50);
  EXPECT_EQ(vendor.capacity(), 100);
  EXPECT_EQ(vendor.available(), 50);
  EXPECT_EQ(vendor.inflight(), 50);

  permits.clear();
  EXPECT_EQ(vendor.capacity(), 100);
  EXPECT_EQ(vendor.available(), 100);
  EXPECT_EQ(vendor.inflight(), 0);
};

TEST(RequestPermitVendorTest, DeletedVendorWithOutstandingPermit) {
  auto vendor = std::make_unique<RequestPermitVendor>(1);
  EXPECT_EQ(vendor->capacity(), 1);
  EXPECT_EQ(vendor->available(), 1);
  EXPECT_EQ(vendor->inflight(), 0);

  auto p1 = vendor->acquirePermit();
  EXPECT_NE(p1, nullptr);
  EXPECT_EQ(vendor->capacity(), 1);
  EXPECT_EQ(vendor->available(), 0);
  EXPECT_EQ(vendor->inflight(), 1);

  EXPECT_NO_THROW(vendor.reset());
  EXPECT_NO_THROW(p1.reset());
};

TEST(RequestPermitVendorTest, PermitsAreMovable) {
  auto vendor = std::make_unique<RequestPermitVendor>(2);
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 2);
  EXPECT_EQ(vendor->inflight(), 0);

  auto p1 = vendor->acquirePermit();
  EXPECT_NE(p1, nullptr);
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 1);
  EXPECT_EQ(vendor->inflight(), 1);

  // Moving the permit should not increase the number of inflight requests
  auto p1_moved = std::move(p1);
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 1);
  EXPECT_EQ(vendor->inflight(), 1);

  // Resetting the original p1 pointer doesn't affect p1_moved
  p1.reset();
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 1);
  EXPECT_EQ(vendor->inflight(), 1);

  // We should be able to acquire a second permit
  auto p2 = vendor->acquirePermit();
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 0);
  EXPECT_EQ(vendor->inflight(), 2);

  // Destroying the first permit should be observable by the vendor
  p1_moved.reset();
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 1);
  EXPECT_EQ(vendor->inflight(), 1);

  p2.reset();
  EXPECT_EQ(vendor->capacity(), 2);
  EXPECT_EQ(vendor->available(), 2);
  EXPECT_EQ(vendor->inflight(), 0);
};
