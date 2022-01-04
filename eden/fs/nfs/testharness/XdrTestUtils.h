/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <fmt/core.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/portability/GTest.h>
#include "eden/fs/nfs/xdr/Xdr.h"

namespace facebook::eden {

using folly::IOBuf;

template <typename T>
std::unique_ptr<IOBuf> ser(const T& t) {
  constexpr size_t kDefaultBufferSize = 1024;
  folly::IOBufQueue buf;
  folly::io::QueueAppender appender(&buf, kDefaultBufferSize);
  XdrTrait<T>::serialize(appender, t);
  return buf.move();
}

template <typename T>
T de(std::unique_ptr<IOBuf> buf) {
  folly::io::Cursor cursor(buf.get());
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
void roundtrip(T value) {
  auto encodedSize = XdrTrait<T>::serializedSize(value);
  auto encoded = ser(value);
  EXPECT_EQ(encoded->coalesce().size(), encodedSize);
  auto decoded = de<T>(std::move(encoded));
  EXPECT_EQ(value, decoded);
}

} // namespace facebook::eden

#endif
