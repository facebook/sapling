/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/EdenError.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(EdenError, recognizeNetworkError) {
  sapling::SaplingBackingStoreError ex1(
      "Network Error: [28] Timeout was reached (Operation too slow. Less than 1500 bytes/sec transferred the last 10 seconds)");
  auto err = newEdenError(ex1);
  EXPECT_TRUE(err.errorCode().has_value());
  EXPECT_EQ(28, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: [28] Timeout was reached"));

  sapling::SaplingBackingStoreError ex2(
      "Network Error: server responded 503 Service Unavailable for some.url");
  err = newEdenError(ex2);
  EXPECT_TRUE(err.errorCode().has_value());
  EXPECT_EQ(503, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find(
          "Network Error: server responded 503 Service Unavailable"));
}

TEST(EdenError, fallbackFromSaplingBackingStoreError) {
  // SaplingBackingStoreError does not contain a network error pattern
  sapling::SaplingBackingStoreError ex1(
      "Generic fetch error without network code");
  auto err = newEdenError(ex1);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Generic fetch error without network code"));

  // Malformed network error pattern
  sapling::SaplingBackingStoreError ex2("Network Error: [404 Not Found");
  err = newEdenError(ex2);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos, err.message()->find("Network Error: [404 Not Found"));

  sapling::SaplingBackingStoreError ex3(
      "Network Error: server responded NON_DIGITS");
  err = newEdenError(ex3);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: server responded NON_DIGITS"));
}

TEST(EdenError, saplingBackingStoreErrorInExceptionWrapper) {
  // Test SaplingBackingStoreError with network error wrapped in
  // exception_wrapper
  sapling::SaplingBackingStoreError ex1(
      "Network Error: [28] Timeout was reached");
  folly::exception_wrapper ew1 =
      folly::make_exception_wrapper<sapling::SaplingBackingStoreError>(ex1);
  auto err = newEdenError(ew1);

  EXPECT_TRUE(err.errorCode().has_value());
  EXPECT_EQ(28, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: [28] Timeout was reached"));

  // Test SaplingBackingStoreError without network error wrapped in
  // exception_wrapper
  sapling::SaplingBackingStoreError ex2("Generic sapling fetch failure");
  folly::exception_wrapper ew2 =
      folly::make_exception_wrapper<sapling::SaplingBackingStoreError>(ex2);
  err = newEdenError(ew2);

  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos, err.message()->find("Generic sapling fetch failure"));
}

TEST(EdenError, rocksException) {
  const rocksdb::Status noSpaceStatus = rocksdb::Status::NoSpace();
  RocksException ex =
      RocksException::build(noSpaceStatus, "Some error message");
  auto err = newEdenError(ex);
  EXPECT_TRUE(err.errorCode().has_value());
#ifdef _WIN32
  EXPECT_EQ(ERROR_DISK_FULL, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::WIN32_ERROR, err.errorType().value());
#else
  EXPECT_EQ(ENOSPC, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::POSIX_ERROR, err.errorType().value());
#endif
  EXPECT_NE(std::string::npos, err.message()->find("Some error message"));

  // Test RocksException wrapped in exception_wrapper
  folly::exception_wrapper ew =
      folly::make_exception_wrapper<RocksException>(ex);
  err = newEdenError(ew);
  EXPECT_TRUE(err.errorCode().has_value());
#ifdef _WIN32
  EXPECT_EQ(ERROR_DISK_FULL, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::WIN32_ERROR, err.errorType().value());
#else
  EXPECT_EQ(ENOSPC, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::POSIX_ERROR, err.errorType().value());
#endif
  EXPECT_NE(std::string::npos, err.message()->find("Some error message"));

  // Test fallback to generic error for uninteresting RocksException
  const rocksdb::Status status = rocksdb::Status::Incomplete();
  ex = RocksException::build(status, "Some error message");
  err = newEdenError(ex);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(std::string::npos, err.message()->find("Some error message"));
}
