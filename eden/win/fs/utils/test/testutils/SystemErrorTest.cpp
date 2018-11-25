/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <iostream>
#include <string>
#include "eden/win/fs/utils/WinError.h"
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
