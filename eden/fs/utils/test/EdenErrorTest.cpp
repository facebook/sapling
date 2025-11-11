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
  sapling::SaplingBackingStoreError ex1{
      "Network Error: some error message",
      sapling::BackingStoreErrorKind::Network,
      28};
  auto err = newEdenError(ex1);
  EXPECT_TRUE(err.errorCode().has_value());
  EXPECT_EQ(28, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: some error message"));

  sapling::SaplingBackingStoreError ex2{
      "Network Error: some other error message",
      sapling::BackingStoreErrorKind::Network,
      std::nullopt};
  err = newEdenError(ex2);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: some other error message"));
}

TEST(EdenError, fallbackFromSaplingBackingStoreError) {
  // SaplingBackingStoreError does not contain a network error
  sapling::SaplingBackingStoreError ex1{
      "Error: some generic error message",
      sapling::BackingStoreErrorKind::Generic,
      std::nullopt};
  auto err = newEdenError(ex1);
  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Error: some generic error message"));
}

TEST(EdenError, saplingBackingStoreErrorInExceptionWrapper) {
  // Test SaplingBackingStoreError with network error wrapped in
  // exception_wrapper
  sapling::SaplingBackingStoreError ex1{
      "Network Error: some error message",
      sapling::BackingStoreErrorKind::Network,
      28};
  folly::exception_wrapper ew1 =
      folly::make_exception_wrapper<sapling::SaplingBackingStoreError>(ex1);
  auto err = newEdenError(ew1);

  EXPECT_TRUE(err.errorCode().has_value());
  EXPECT_EQ(28, err.errorCode().value());
  EXPECT_EQ(EdenErrorType::NETWORK_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Network Error: some error message"));

  // Test SaplingBackingStoreError without network error wrapped in
  // exception_wrapper
  sapling::SaplingBackingStoreError ex2{
      "Error: some generic error message",
      sapling::BackingStoreErrorKind::Generic,
      std::nullopt};
  folly::exception_wrapper ew2 =
      folly::make_exception_wrapper<sapling::SaplingBackingStoreError>(ex2);
  err = newEdenError(ew2);

  EXPECT_FALSE(err.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, err.errorType().value());
  EXPECT_NE(
      std::string::npos,
      err.message()->find("Error: some generic error message"));
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
