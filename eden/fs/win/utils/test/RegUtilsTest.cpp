/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/utils/RegUtils.h"
#include <filesystem>
#include <string>
#include "eden/fs/win/utils/Guid.h"
#include "gtest/gtest.h"

using namespace facebook::eden;

namespace {
class RegUtilsTest : public ::testing::Test {
 protected:
  void SetUp() override {
    rootPath_ /= guid_.toWString();
    rootKey_ = RegistryKey::createCurrentUser(rootPath_.c_str());
  }

  void TearDown() override {
    rootKey_.deleteKey();
  }

  Guid guid_ = {Guid::generate()};
  RegistryKey rootKey_;
  std::filesystem::path rootPath_{L"software\\facebook\\test"};
};
} // namespace

TEST_F(RegUtilsTest, testRegCreateandEnumerate) {
  const auto path = rootPath_ / L"testRegCreateandEnumerate";
  auto key1{RegistryKey::createCurrentUser(path.c_str())};
  key1.create(L"Key1");
  key1.create(L"Key2");
  key1.create(L"Key3");
  key1.create(L"Key4");
  key1.create(L"Key5");

  auto entries = key1.enumerateKeys();
  EXPECT_EQ(5, entries.size());

  EXPECT_EQ(L"Key1", entries.at(0));
  EXPECT_EQ(L"Key2", entries.at(1));
  EXPECT_EQ(L"Key3", entries.at(2));
  EXPECT_EQ(L"Key4", entries.at(3));
  EXPECT_EQ(L"Key5", entries.at(4));

  key1.deleteKey();
  EXPECT_ANY_THROW(RegistryKey::openCurrentUser(path.c_str()));
}

TEST_F(RegUtilsTest, testRegOpen) {
  const auto path = rootPath_ / L"testRegOpen";
  auto rootkey = RegistryKey::create(HKEY_CURRENT_USER, path.c_str());

  rootkey.create(L"Key1");
  auto key1Path = path / L"key1";

  auto key1 = RegistryKey::openCurrentUser(key1Path.c_str());
  rootkey.deleteKey();
  // Test succeeds if no exception is thrown
}

TEST_F(RegUtilsTest, testRegValues) {
  const auto path = rootPath_ / L"testRegValues";
  DWORD dwordValue = 1010;
  std::wstring stringValue = L"This is a test string";
  BYTE binaryData[] = "Binary data test";
  BYTE binaryDataResult[1024];
  auto rootkey = RegistryKey::createCurrentUser(path.c_str());

  rootkey.setDWord(L"value1", dwordValue);
  rootkey.setString(L"value2", stringValue);
  rootkey.setBinary(L"value3", binaryData, sizeof(binaryData));

  EXPECT_EQ(dwordValue, rootkey.getDWord(L"value1"));
  EXPECT_EQ(stringValue, rootkey.getString(L"value2"));

  auto size =
      rootkey.getBinary(L"value3", binaryDataResult, sizeof(binaryDataResult));
  EXPECT_EQ(sizeof(binaryData), size);
  EXPECT_EQ(0, memcmp(binaryDataResult, binaryData, sizeof(binaryData)));

  auto entries = rootkey.enumerateValues();
  EXPECT_EQ(3, entries.size());
  EXPECT_EQ(entries.at(0).first, L"value1");
  EXPECT_EQ(entries.at(1).first, L"value2");
  EXPECT_EQ(entries.at(2).first, L"value3");

  rootkey.deleteValue(L"value1");
  entries = rootkey.enumerateValues();
  EXPECT_EQ(2, entries.size());

  rootkey.deleteValue(L"value3");
  entries = rootkey.enumerateValues();
  EXPECT_EQ(1, entries.size());

  rootkey.deleteValue(L"value2");
  entries = rootkey.enumerateValues();
  EXPECT_EQ(0, entries.size());

  rootkey.deleteKey();
}

TEST_F(RegUtilsTest, testRenameKey) {
  const auto rootPath = rootPath_ / L"testRenameKey";
  auto testPath{rootPath / L"Testkey"};
  std::wstring newName1{L"Newkey1"};
  auto newPath1{rootPath / newName1};
  std::wstring newName2{L"Newkey2"};

  DWORD dwordValue = 1010;
  std::wstring stringValue = L"This is a test string";
  BYTE binaryData[] = "Binary data test";
  BYTE binaryDataResult[1024];
  {
    auto testKey = RegistryKey::createCurrentUser(testPath.c_str());

    testKey.setDWord(L"value1", dwordValue);
    testKey.setString(L"value2", stringValue);
    testKey.setBinary(L"value3", binaryData, sizeof(binaryData));

    EXPECT_EQ(dwordValue, testKey.getDWord(L"value1"));
    EXPECT_EQ(stringValue, testKey.getString(L"value2"));

    auto size = testKey.getBinary(
        L"value3", binaryDataResult, sizeof(binaryDataResult));
    EXPECT_EQ(sizeof(binaryData), size);
    EXPECT_EQ(0, memcmp(binaryDataResult, binaryData, sizeof(binaryData)));

    auto entries = testKey.enumerateValues();
    EXPECT_EQ(3, entries.size());
    EXPECT_EQ(entries.at(0).first, L"value1");
    EXPECT_EQ(entries.at(1).first, L"value2");
    EXPECT_EQ(entries.at(2).first, L"value3");
  }

  RegistryKey::renameKey(HKEY_CURRENT_USER, newName1.c_str(), testPath.c_str());
  RegistryKey::renameKey(HKEY_CURRENT_USER, newName2.c_str(), newPath1.c_str());
}
