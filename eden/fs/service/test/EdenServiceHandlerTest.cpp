/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cerrno>
#include <limits>
#include <memory>
#include <string>

#include <gtest/gtest.h>

#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/sl/SaplingBackingStore.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h"

namespace facebook::eden {

// Declared with external linkage in EdenServiceHandler.cpp specifically so
// they can be exercised directly here - see the "visible for testing"
// comment at their definition.
CacheUsageState toThriftCacheUsageState(sapling::CacheUsageState state);
int64_t toThriftByteCount(uint64_t value);
int64_t toThriftByteLimit(uint64_t value);
std::shared_ptr<SaplingBackingStore> tryCastToSaplingBackingStore(
    std::shared_ptr<BackingStore>& backingStore);
void validateInjectFaultDefinition(const FaultDefinition& fault);

void expectInjectFaultArgumentError(const FaultDefinition& fault) {
  try {
    validateInjectFaultDefinition(fault);
    FAIL() << "expected inject fault validation to fail";
  } catch (const EdenError& error) {
    EXPECT_EQ(EINVAL, error.errorCode().value());
    EXPECT_EQ(EdenErrorType::ARGUMENT_ERROR, error.errorType().value());
    EXPECT_NE(
        std::string::npos,
        error.message()->find(
            "unblockKeyClass and unblockKeyValueRegex must be set together"));
  }
}

TEST(EdenServiceHandlerTest, toThriftCacheUsageStateMapsAllStates) {
  EXPECT_EQ(
      CacheUsageState::NOT_CONFIGURED,
      toThriftCacheUsageState(sapling::CacheUsageState::NotConfigured));
  EXPECT_EQ(
      CacheUsageState::UNSUPPORTED,
      toThriftCacheUsageState(sapling::CacheUsageState::Unsupported));
  EXPECT_EQ(
      CacheUsageState::AVAILABLE,
      toThriftCacheUsageState(sapling::CacheUsageState::Available));
  EXPECT_EQ(
      CacheUsageState::UNAVAILABLE,
      toThriftCacheUsageState(sapling::CacheUsageState::Unavailable));
}

TEST(EdenServiceHandlerTest, toThriftByteCountPassesThroughNormalValues) {
  EXPECT_EQ(0, toThriftByteCount(0));
  EXPECT_EQ(12345, toThriftByteCount(12345));
  EXPECT_EQ(
      std::numeric_limits<int64_t>::max(),
      toThriftByteCount(
          static_cast<uint64_t>(std::numeric_limits<int64_t>::max())));
}

TEST(EdenServiceHandlerTest, toThriftByteCountClampsValuesBeyondInt64Max) {
  // A byte *count* has no "uncapped" sentinel - even u64::MAX must clamp
  // like any other too-large value, never map to -1 (that would make an
  // implausibly large used-count look like a healthy "uncapped" limit).
  EXPECT_EQ(
      std::numeric_limits<int64_t>::max(),
      toThriftByteCount(std::numeric_limits<uint64_t>::max()));
  uint64_t tooLarge =
      static_cast<uint64_t>(std::numeric_limits<int64_t>::max()) + 1;
  EXPECT_EQ(std::numeric_limits<int64_t>::max(), toThriftByteCount(tooLarge));
}

TEST(EdenServiceHandlerTest, toThriftByteLimitMapsUncappedSentinel) {
  // u64::MAX is the "uncapped" sentinel produced by the Rust FFI layer
  // (to_ffi_cache_usage in ffi.rs) and must map explicitly to Thrift's -1,
  // not rely on static_cast<int64_t> silently producing -1 by luck.
  EXPECT_EQ(-1, toThriftByteLimit(std::numeric_limits<uint64_t>::max()));
}

TEST(EdenServiceHandlerTest, toThriftByteLimitPassesThroughNormalValues) {
  EXPECT_EQ(0, toThriftByteLimit(0));
  EXPECT_EQ(12345, toThriftByteLimit(12345));
}

TEST(EdenServiceHandlerTest, toThriftByteLimitClampsValuesBeyondInt64Max) {
  // Not u64::MAX (the uncapped sentinel), but still too large for int64_t -
  // must clamp defensively rather than silently become negative.
  uint64_t tooLarge =
      static_cast<uint64_t>(std::numeric_limits<int64_t>::max()) + 1;
  EXPECT_EQ(std::numeric_limits<int64_t>::max(), toThriftByteLimit(tooLarge));
}

TEST(
    EdenServiceHandlerTest,
    validateInjectFaultDefinitionRequiresCompleteUnblockTarget) {
  FaultDefinition onlyClass;
  onlyClass.unblockKeyClass() = "blocked";
  expectInjectFaultArgumentError(onlyClass);

  FaultDefinition onlyRegex;
  onlyRegex.unblockKeyValueRegex() = ".*";
  expectInjectFaultArgumentError(onlyRegex);

  FaultDefinition bothFields;
  bothFields.unblockKeyClass() = "blocked";
  bothFields.unblockKeyValueRegex() = ".*";
  EXPECT_NO_THROW(validateInjectFaultDefinition(bothFields));
}

TEST(
    EdenServiceHandlerTest,
    tryCastToSaplingBackingStoreReturnsNullForNonSaplingStore) {
  // populateHgCacheStats() relies on this cast to detect non-Sapling mounts
  // (e.g. Git or RE-CAS) and skip hgcache stats for them as an expected,
  // benign case rather than an error. Testing the cast directly here (rather
  // than via a full EdenMount/TestMount) keeps this test independent of
  // TestMount's user/passwd lookup, which some sandboxed environments can't
  // satisfy.
  std::shared_ptr<BackingStore> backingStore =
      std::make_shared<FakeBackingStore>();

  EXPECT_EQ(nullptr, tryCastToSaplingBackingStore(backingStore));
}

} // namespace facebook::eden
