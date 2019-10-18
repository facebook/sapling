/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/utils/Guid.h"
#include <iostream>
#include <string>
#include "gtest/gtest.h"

using namespace facebook::eden;

TEST(GuidTest, assignedGuid) {
  // {811305DA-F51E-4E2D-9201-0D12A1E7F8D5}
  static const GUID testGuid = {0x811305da,
                                0xf51e,
                                0x4e2d,
                                {0x92, 0x1, 0xd, 0x12, 0xa1, 0xe7, 0xf8, 0xd5}};

  Guid guid{testGuid};
  std::wstring guidWString{L"{811305DA-F51E-4E2D-9201-0D12A1E7F8D5}"};
  std::string guidString{"{811305DA-F51E-4E2D-9201-0D12A1E7F8D5}"};

  EXPECT_EQ(guid.toWString(), guidWString);
  EXPECT_EQ(guid.toString(), guidString);
  EXPECT_EQ(guid.getGuid(), testGuid);
}

TEST(GuidTest, emptyGuid) {
  static const GUID testGuid{0};
  std::wstring guidWString{L"{00000000-0000-0000-0000-000000000000}"};
  std::string guidString{"{00000000-0000-0000-0000-000000000000}"};
  Guid guid;

  EXPECT_EQ(guid.toWString(), guidWString);
  EXPECT_EQ(guid.toString(), guidString);
  EXPECT_EQ(guid.getGuid(), testGuid);
}

TEST(GuidTest, generatedGuid) {
  Guid guid;
  // Use Assignment operator
  guid = Guid::generate();
  Guid testGuid{guid};

  EXPECT_EQ(testGuid.toWString(), guid.toWString());
  EXPECT_EQ(guid.getGuid(), testGuid.getGuid());
}

TEST(GuidTest, compareGuids) {
  // {811305DA-F51E-4E2D-9201-0D12A1E7F8D5}
  static const GUID testGuid = {0x811305da,
                                0xf51e,
                                0x4e2d,
                                {0x92, 0x1, 0xd, 0x12, 0xa1, 0xe7, 0xf8, 0xd5}};

  Guid guid1{testGuid};
  Guid guid2;
  Guid guid3;
  Guid guid4{Guid::generate()};

  guid2 = testGuid;

  EXPECT_EQ(guid1, guid2);
  EXPECT_NE(guid1, guid3);
  EXPECT_NE(guid1, guid4);
}

TEST(GuidTest, pointerGuids) {
  // {811305DA-F51E-4E2D-9201-0D12A1E7F8D5}
  static const GUID testGuid = {0x811305da,
                                0xf51e,
                                0x4e2d,
                                {0x92, 0x1, 0xd, 0x12, 0xa1, 0xe7, 0xf8, 0xd5}};

  Guid guid1{testGuid};
  const GUID* ptrGuid1 = guid1;
  const GUID* ptrGuid2 = &testGuid;
  Guid guid2;
  Guid guid3{*ptrGuid2};

  guid2 = *ptrGuid1;

  EXPECT_EQ(guid1, guid2);
  EXPECT_EQ(guid1, guid3);
}
