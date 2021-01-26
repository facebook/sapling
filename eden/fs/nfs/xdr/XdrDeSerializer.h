/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/io/Cursor.h>
#include "eden/fs/nfs/xdr/XdrSerializer.h"

namespace facebook::eden {

// https://tools.ietf.org/html/rfc4506
class XdrDeSerializer : public folly::io::Cursor {
 public:
  using folly::io::Cursor::Cursor;

  int32_t xdr_integer();
  uint32_t xdr_integer_unsigned();

  int64_t xdr_hyper_integer();
  uint64_t xdr_hyper_integer_unsigned();
  bool xdr_bool();
  float xdr_float();
  double xdr_double();
};

// Code can assume that `void deSerializeXdrInto(XdrSerializer&, T&)` is defined
// to a type T to deserialize it from Xdr representation.
// This header provides some basic/generic implementations.
// It is expected that other code will define that function for structs
// that they define and wish to deserialize.

void deSerializeXdrInto(XdrDeSerializer& xdr, int32_t& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, uint32_t& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, int64_t& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, uint64_t& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, bool& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, float& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, double& value);
void deSerializeXdrInto(XdrDeSerializer& xdr, std::string& result);
void deSerializeXdrInto(XdrDeSerializer& xdr, std::vector<uint8_t>& result);

template <class T>
typename std::enable_if<std::is_enum<T>::value>::type deSerializeXdrInto(
    XdrDeSerializer& xdr,
    T& value) {
  // Decode the signed integer value
  int32_t intValue;
  deSerializeXdrInto(xdr, intValue);
  // And hope that it does actually match up to the defined enum
  // variants in the code(!)
  value = static_cast<T>(intValue);
}

// std::array is decoded as a fixed size array based on the size (N)
// of the array type.  There is no preceding length indicator.
template <class T, size_t N>
void deSerializeXdrInto(XdrDeSerializer& xdr, std::array<T, N>& result) {
  for (auto& item : result) {
    deSerializeXdrInto(xdr, item);
  }
}

// std::vector is decoded as a variable size array; we decode the
// length indicator and then decode that number of `T`'s.
template <class T>
void deSerializeXdrInto(XdrDeSerializer& xdr, std::vector<T>& result) {
  auto len = xdr.xdr_integer_unsigned();
  result.resize(len);
  for (auto& item : result) {
    deSerializeXdrInto(xdr, item);
  }
}

} // namespace facebook::eden
