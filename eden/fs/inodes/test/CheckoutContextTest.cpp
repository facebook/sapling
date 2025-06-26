/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>

#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

namespace facebook::eden {
TEST(CheckoutContextTest, empty) {
  auto builder1 = FakeTreeBuilder();
  TestMount testMount{builder1};

  bool verifyFilesAfterCheckout = true;
  size_t verifyEveryNInvalidations = 3;
  size_t maxNumberOfInvlidationsToValidate = 5;

  auto ctx = CheckoutContext{
      testMount.getEdenMount().get(),
      CheckoutMode::NORMAL,
      OptionalProcessId{},
      "checkout_context_test",
      verifyFilesAfterCheckout,
      verifyEveryNInvalidations,
      maxNumberOfInvlidationsToValidate};

  ctx.maybeRecordInvalidation(InodeNumber{1});
  ctx.maybeRecordInvalidation(InodeNumber{2});
  ctx.maybeRecordInvalidation(InodeNumber{3});
  ctx.maybeRecordInvalidation(InodeNumber{4});
  ctx.maybeRecordInvalidation(InodeNumber{5});

  auto result = ctx.extractFilesToVerify();
  EXPECT_EQ(5, result.size());
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{1}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{2}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{3}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{4}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{5}));
}

TEST(CheckoutContextTest, overMax) {
  auto builder1 = FakeTreeBuilder();
  TestMount testMount{builder1};

  bool verifyFilesAfterCheckout = true;
  size_t verifyEveryNInvalidations = 3;
  size_t maxNumberOfInvlidationsToValidate = 5;

  auto ctx = CheckoutContext{
      testMount.getEdenMount().get(),
      CheckoutMode::NORMAL,
      OptionalProcessId{},
      "checkout_context_test",
      verifyFilesAfterCheckout,
      verifyEveryNInvalidations,
      maxNumberOfInvlidationsToValidate};

  ctx.maybeRecordInvalidation(InodeNumber{1}); // added
  ctx.maybeRecordInvalidation(InodeNumber{2}); // added
  ctx.maybeRecordInvalidation(InodeNumber{3}); // added
  ctx.maybeRecordInvalidation(InodeNumber{4}); // added
  ctx.maybeRecordInvalidation(InodeNumber{5}); // added
  ctx.maybeRecordInvalidation(InodeNumber{6}); // skipped
  ctx.maybeRecordInvalidation(InodeNumber{7}); // added removes 1
  ctx.maybeRecordInvalidation(InodeNumber{8}); // skipped
  ctx.maybeRecordInvalidation(InodeNumber{9}); // skipped
  ctx.maybeRecordInvalidation(InodeNumber{10}); // added removes 2

  auto result = ctx.extractFilesToVerify();
  EXPECT_EQ(5, result.size());

  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{7}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{10}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{3}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{4}));
  EXPECT_NE(
      result.end(), std::find(result.begin(), result.end(), InodeNumber{5}));
}
} // namespace facebook::eden
