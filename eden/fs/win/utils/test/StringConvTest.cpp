/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/utils/StringConv.h"
#include <string>
#include "gtest/gtest.h"
using namespace facebook::eden;

TEST(StringConvTest, testWintoEdenPath) {
  std::wstring winPath = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "C:/winPath/PATH1/path/File.txt";
  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPath) {
  std::wstring winPath = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "C:/winPath/PATH1/path/File.txt";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}

TEST(StringConvTest, testWintoEdenPathWithEmptyString) {
  std::wstring winPath = L"";
  std::string edenPath = "";

  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathWithEmptyString) {
  std::wstring winPath = L"";
  std::string edenPath = "";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}

TEST(StringConvTest, testWintoEdenPathWithLongString) {
  std::wstring winPath =
      L"C:\\winPath\\PATHaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\path\\File.txt";
  std::string edenPath =
      "C:/winPath/PATHaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaa/path/File.txt";

  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathWithLongString) {
  std::wstring winPath =
      L"C:\\winPath\\PATHaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\path\\File.txt";
  std::string edenPath =
      "C:/winPath/PATHaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      "aaaaaaaaaaaaaaaaaaaaaaaaaaaaa/path/File.txt";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}

TEST(StringConvTest, testWintoEdenPathComponent) {
  std::wstring winPath = L"LongFileName.txt";
  std::string edenPath = "LongFileName.txt";

  EXPECT_EQ(winToEdenName(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathComponent) {
  std::wstring winPath = L"LongFileName.txt";
  std::string edenPath = "LongFileName.txt";

  EXPECT_EQ(edenToWinName(edenPath), winPath);
}

TEST(StringConvTest, testWinToEdenToWinPathRoundTrips) {
  std::wstring winPath = L"\\winPath\\PATH1\\path\\File.txt";
  std::string edenPath = winToEdenPath(winPath);
  std::wstring newWinPath = edenToWinPath(edenPath);
  EXPECT_EQ(winPath, newWinPath);
}

TEST(StringConvTest, testEdenToWinToEdenPathRoundTrips) {
  std::string edenPath = "/winPath/PATH1/path/File.txt";
  std::wstring winPath = edenToWinPath(edenPath);
  std::string newedenPath = winToEdenPath(winPath);
  EXPECT_EQ(newedenPath, edenPath);
}

TEST(StringConvTest, testWstringToString) {
  std::wstring wideStr = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string str = "C:\\winPath\\PATH1\\path\\File.txt";

  EXPECT_EQ(wstringToString(wideStr), str);
}

TEST(StringConvTest, testStringToWstring) {
  std::wstring wideStr = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string str = "C:\\winPath\\PATH1\\path\\File.txt";

  EXPECT_EQ(stringToWstring(str), wideStr);
}

TEST(StringConvTest, testWcharToString) {
  std::wstring wideStr = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string str = "C:\\winPath\\PATH1\\path\\File.txt";

  EXPECT_EQ(wcharToString(wideStr.c_str()), str);
}

TEST(StringConvTest, testcharToWstring) {
  std::wstring wideStr = L"C:\\winPath\\PATH1\\path\\File.txt";
  std::string str = "C:\\winPath\\PATH1\\path\\File.txt";

  EXPECT_EQ(charToWstring(str.c_str()), wideStr);
}

TEST(StringConvTest, testWcharToStringWithNullptr) {
  std::string str = "";

  EXPECT_EQ(wcharToString(nullptr), str);
}

TEST(StringConvTest, testcharToWstringWithNullptr) {
  std::wstring wideStr = L"";

  std::wstring newWStr = charToWstring(nullptr);
  EXPECT_EQ(newWStr, wideStr);
}

TEST(StringConvTest, testWcharToStringWithEmptyPath) {
  std::wstring wideStr = L"";
  std::string str = "";

  std::string newStr = wcharToString(wideStr.c_str());
  EXPECT_EQ(newStr, str);
}

TEST(StringConvTest, testcharToWstringWithEmptyPath) {
  std::wstring wideStr = L"";
  std::string str = "";

  std::wstring newWStr = charToWstring(str.c_str());
  EXPECT_EQ(newWStr, wideStr);
}

TEST(StringConvTest, testWintoEdenPathRelativePath) {
  std::wstring winPath = L"winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "winPath/PATH1/path/File.txt";
  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathRelativePath) {
  std::wstring winPath = L"winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "winPath/PATH1/path/File.txt";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}

TEST(StringConvTest, testWintoEdenPathMixedPath) {
  std::wstring winPath = L"mixed/winPath\\PATH1/path\\File.txt";
  std::string edenPath = "mixed/winPath/PATH1/path/File.txt";
  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathMixedPath) {
  std::wstring winPath = L"winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "winPath/PATH1\\path/File.txt";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}

TEST(StringConvTest, testWintoEdenPathNTPath) {
  std::wstring winPath = L"\\??\\mixed\\winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "/??/mixed/winPath/PATH1/path/File.txt";

  EXPECT_EQ(winToEdenPath(winPath), edenPath);
}

TEST(StringConvTest, testEdenToWinPathNTPath) {
  std::wstring winPath = L"\\??\\mixed\\winPath\\PATH1\\path\\File.txt";
  std::string edenPath = "/??/mixed/winPath/PATH1/path/File.txt";

  EXPECT_EQ(edenToWinPath(edenPath), winPath);
}
