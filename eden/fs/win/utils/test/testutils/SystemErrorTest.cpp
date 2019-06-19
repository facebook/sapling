/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <iostream>
#include <string>
#include "eden/fs/win/utils/WinError.h"
#include "gtest/gtest.h"
using namespace facebook::eden;

// Test Win32 error
TEST(WinErrorTest, testErrorFileNotFound) {
  std::string msg{
      "Error ERROR_FILE_NOT_FOUND: Error (0x2) The system cannot find the"
      " file specified.\r\n"};
  auto ex = std::system_error(
      ERROR_FILE_NOT_FOUND,
      Win32ErrorCategory::get(),
      "Error ERROR_FILE_NOT_FOUND");

  EXPECT_EQ(msg, ex.what());
}

// Test Win32 success
TEST(WinErrorTest, testErrorSuccess) {
  std::string msg{
      "Error ERROR_SUCCESS: Error (0x0) The operation completed successfully.\r\n"};
  auto ex = std::system_error(
      ERROR_SUCCESS, Win32ErrorCategory::get(), "Error ERROR_SUCCESS");

  EXPECT_EQ(msg, ex.what());
}

// Test HRESULT error
TEST(WinErrorTest, testErrorConfigNotFound) {
  std::string msg{
      "Error NAP_E_SHV_CONFIG_NOT_FOUND: Error (0x80270012) SHV configuration"
      " is not found.\r\n"};
  auto ex = std::system_error(
      NAP_E_SHV_CONFIG_NOT_FOUND,
      HResultErrorCategory::get(),
      "Error NAP_E_SHV_CONFIG_NOT_FOUND");

  EXPECT_EQ(msg, ex.what());
}

// Test HRESULT success
TEST(WinErrorTest, testErrorSOK) {
  std::string msg{
      "Error S_OK: Error (0x0) The operation completed successfully.\r\n"};
  auto ex = std::system_error(S_OK, HResultErrorCategory::get(), "Error S_OK");

  EXPECT_EQ(msg, ex.what());
}

// Test Invalid error code
TEST(WinErrorTest, testErrorInvalidCode) {
  std::string msg{"Error Invalid code: Error (0x22222222) Unknown Error\r\n"};
  auto ex = std::system_error(
      0x22222222, Win32ErrorCategory::get(), "Error Invalid code");

  EXPECT_EQ(msg, ex.what());
}

//
// Test exceptionToHResultWrapper, makeHResultError and HResultError exception
//
HRESULT throwHResultError(int arg1, std::string arg2) {
  EXPECT_EQ(arg1, 10);
  EXPECT_EQ(arg2, "TestString");
  throw makeHResultErrorExplicit(E_OUTOFMEMORY, "Test throw");
}

HRESULT catchHResultError(int arg1, std::string arg2) {
  return exceptionToHResultWrapper(
      [&]() { return throwHResultError(arg1, arg2); });
}

TEST(WinErrorTest, testexceptionToHResultWrapper_E_OUTOFMEMORY) {
  int arg1 = 10;
  std::string arg2 = "TestString";

  EXPECT_EQ(catchHResultError(arg1, arg2), E_OUTOFMEMORY);
}

TEST(WinErrorTest, testexceptionToHResult_E_OUTOFMEMORY) {
  try {
    throw makeHResultErrorExplicit(E_OUTOFMEMORY, "Test throw");
  } catch (...) {
    EXPECT_EQ(exceptionToHResult(), E_OUTOFMEMORY);
  }
}

TEST(WinErrorTest, testexceptionToHResult_ERROR_ACCESS_DENIED) {
  try {
    throw makeWin32ErrorExplicit(ERROR_ACCESS_DENIED, "Test throw");
  } catch (...) {
    EXPECT_EQ(exceptionToHResult(), HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED));
  }
}

//
// Test exceptionToHResult, makeHResultFromWin32Error and HResultError exception
//
HRESULT throwWin32Error(int arg1, std::string arg2) {
  EXPECT_EQ(arg1, 2232);
  EXPECT_EQ(arg2, "Test String Win32");
  throw makeWin32ErrorExplicit(ERROR_FILE_NOT_FOUND, "Test throw");
}

HRESULT catchWin32Error(int arg1, std::string arg2) {
  return exceptionToHResultWrapper(
      [&]() { return throwWin32Error(arg1, arg2); });
}

TEST(WinErrorTest, testexceptionToHResultWrapper_ERROR_FILE_NOT_FOUND) {
  int arg1 = 2232;
  std::string arg2 = "Test String Win32";

  EXPECT_EQ(
      catchWin32Error(arg1, arg2), HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
}

TEST(WinErrorTest, testexceptionToHResult_ERROR_FILE_NOT_FOUND) {
  try {
    throw makeWin32ErrorExplicit(ERROR_FILE_NOT_FOUND, "Test throw");
  } catch (...) {
    EXPECT_EQ(exceptionToHResult(), HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
  }
}

//
// Test exceptionToHResultWrapper, with system_error and HResultError exception
//
HRESULT throwSystemError(int arg1, std::string arg2) {
  EXPECT_EQ(arg1, 1111);
  EXPECT_EQ(arg2, "Test String Win32");

  throw std::system_error(EEXIST, std::generic_category(), "Test Throw");
}

HRESULT catchSystemError(int arg1, std::string arg2) {
  return exceptionToHResultWrapper(
      [&]() { return throwSystemError(arg1, arg2); });
}

TEST(WinErrorTest, testexceptionToHResultWrapper_EACCES) {
  int arg1 = 1111;
  std::string arg2 = "Test String Win32";

  EXPECT_EQ(catchSystemError(arg1, arg2), ERROR_ERRORS_ENCOUNTERED);
}

TEST(WinErrorTest, testexceptionToHResult_EACCES) {
  try {
    throw std::system_error(EEXIST, std::generic_category(), "Test Throw");
  } catch (...) {
    EXPECT_EQ(exceptionToHResult(), ERROR_ERRORS_ENCOUNTERED);
  }
}
