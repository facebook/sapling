/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <fmt/core.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/nfs/xdr/Xdr.h"

namespace facebook::eden {

template <typename T>
folly::IOBuf ser(const T& t) {
  constexpr size_t kDefaultBufferSize = 1024;
  folly::IOBuf buf(folly::IOBuf::CREATE, kDefaultBufferSize);
  folly::io::Appender appender(&buf, kDefaultBufferSize);
  XdrTrait<T>::serialize(appender, t);
  return buf;
}

template <typename T>
T de(folly::IOBuf buf) {
  folly::io::Cursor cursor(&buf);
  auto ret = XdrTrait<T>::deserialize(cursor);
  if (!cursor.isAtEnd()) {
    throw std::runtime_error(fmt::format(
        FMT_STRING("unexpected trailing bytes ({})"), cursor.totalLength()));
  }
  return ret;
}

// Validates that `T` can be serialized into something of an expected
// encoded size and deserialized back to something that compares
// equal to the original value
template <typename T>
void roundtrip(T value, size_t encodedSize) {
  auto encoded = ser(value);
  EXPECT_EQ(encoded.coalesce().size(), encodedSize);
  auto decoded = de<T>(encoded);
  EXPECT_EQ(value, decoded);
}

} // namespace facebook::eden

#endif
