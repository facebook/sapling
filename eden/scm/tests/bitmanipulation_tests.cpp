// Copyright 2004-present Facebook. All Rights Reserved.
#include <folly/portability/GTest.h>

#include <array>
#include "edenscm/mercurial/bitmanipulation.h"

namespace {
// This is basically like std::make_array<char>(), but uses memcpy to avoid
// undefined behavior when handling input bytes that fit in unsigned char
// but not char.
//
// We need to return array<char> rather than array<unsigned char> since the
// functions we are testing expect a 'const char*'
template <typename... Args>
constexpr std::array<char, sizeof...(Args)> make_buf(Args&&... args) {
  std::array<unsigned char, sizeof...(Args)> unsigned_array = {
      {static_cast<unsigned char>(std::forward<Args>(args))...}};
  std::array<char, sizeof...(Args)> result{{}};
  memcpy(result.data(), unsigned_array.data(), unsigned_array.size());
  return result;
}
} // namespace

TEST(BitManipulation, getbe32) {
  EXPECT_EQ(0x12345678, getbe32(make_buf(0x12, 0x34, 0x56, 0x78).data()));

  EXPECT_EQ(0x12345678, getbe32(make_buf(0x12, 0x34, 0x56, 0x78).data()));
  EXPECT_EQ(0xffffffffUL, getbe32(make_buf(0xff, 0xff, 0xff, 0xff).data()));
}

TEST(BitManipulation, putbe32) {
  std::array<char, 4> buf;
  putbe32(0x87654321UL, buf.data());
  EXPECT_EQ(0x87654321UL, getbe32(buf.data()));
  putbe32(0, buf.data());
  EXPECT_EQ(0, getbe32(buf.data()));
  putbe32(42, buf.data());
  EXPECT_EQ(42, getbe32(buf.data()));
}

TEST(BitManipulation, getbeuint16) {
  EXPECT_EQ(0x1234, getbeuint16(make_buf(0x12, 0x34).data()));
  EXPECT_EQ(0xffff, getbeuint16(make_buf(0xff, 0xff).data()));
}

TEST(BitManipulation, getbeint16) {
  EXPECT_EQ(0x1234, getbeint16(make_buf(0x12, 0x34).data()));
  EXPECT_EQ(-1, getbeint16(make_buf(0xff, 0xff).data()));
  EXPECT_EQ(-2, getbeint16(make_buf(0xff, 0xfe).data()));
}

TEST(BitManipulation, getbefloat64) {
  EXPECT_EQ(0.0, getbefloat64(make_buf(0, 0, 0, 0, 0, 0, 0, 0).data()));
  EXPECT_EQ(-0.0, getbefloat64(make_buf(0x80, 0, 0, 0, 0, 0, 0, 0).data()));
  EXPECT_DOUBLE_EQ(
      2.0, getbefloat64(make_buf(0x40, 0, 0, 0, 0, 0, 0, 1).data()));
  EXPECT_DOUBLE_EQ(
      -8.0, getbefloat64(make_buf(0xc0, 0x20, 0, 0, 0, 0, 0, 1).data()));
  EXPECT_DOUBLE_EQ(
      -4.0,
      getbefloat64(
          make_buf(0xc0, 0x0f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff).data()));
}
