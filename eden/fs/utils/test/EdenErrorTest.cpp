/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/EdenError.h"

#include <gtest/gtest.h>

// @lint-ignore-every SPELL

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
  RocksException noSpaceEx =
      RocksException::build(noSpaceStatus, "Some error message");
  auto noSpaceErr = newEdenError(noSpaceEx);
  EXPECT_TRUE(noSpaceErr.errorCode().has_value());
#ifdef _WIN32
  EXPECT_EQ(ERROR_DISK_FULL, noSpaceErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::WIN32_ERROR, noSpaceErr.errorType().value());
#else
  EXPECT_EQ(ENOSPC, noSpaceErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::POSIX_ERROR, noSpaceErr.errorType().value());
#endif
  EXPECT_NE(
      std::string::npos, noSpaceErr.message()->find("Some error message"));

  const rocksdb::Status corruptionStatus = rocksdb::Status::Corruption();
  RocksException corruptionEx =
      RocksException::build(corruptionStatus, "Some error message");
  auto curroptionErr = newEdenError(corruptionEx);
  EXPECT_TRUE(curroptionErr.errorCode().has_value());
#ifdef _WIN32
  EXPECT_EQ(ERROR_FILE_CORRUPT, curroptionErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::WIN32_ERROR, curroptionErr.errorType().value());
#else
  EXPECT_EQ(EBADMSG, curroptionErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::POSIX_ERROR, curroptionErr.errorType().value());
#endif
  EXPECT_NE(
      std::string::npos, curroptionErr.message()->find("Some error message"));

  // Test RocksException wrapped in exception_wrapper
  folly::exception_wrapper ew =
      folly::make_exception_wrapper<RocksException>(noSpaceEx);
  noSpaceErr = newEdenError(ew);
  EXPECT_TRUE(noSpaceErr.errorCode().has_value());
#ifdef _WIN32
  EXPECT_EQ(ERROR_DISK_FULL, noSpaceErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::WIN32_ERROR, noSpaceErr.errorType().value());
#else
  EXPECT_EQ(ENOSPC, noSpaceErr.errorCode().value());
  EXPECT_EQ(EdenErrorType::POSIX_ERROR, noSpaceErr.errorType().value());
#endif
  EXPECT_NE(
      std::string::npos, noSpaceErr.message()->find("Some error message"));

  // Test fallback to generic error for uninteresting RocksException
  const rocksdb::Status boringStatus = rocksdb::Status::Incomplete();
  auto boringEx = RocksException::build(boringStatus, "Some error message");
  auto boringErr = newEdenError(boringEx);
  EXPECT_FALSE(boringErr.errorCode().has_value());
  EXPECT_EQ(EdenErrorType::GENERIC_ERROR, boringErr.errorType().value());
  EXPECT_NE(std::string::npos, boringErr.message()->find("Some error message"));
}
