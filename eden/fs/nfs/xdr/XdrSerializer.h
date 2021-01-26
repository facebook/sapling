/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/Cursor.h>

namespace facebook::eden {

// https://tools.ietf.org/html/rfc4506

class XdrSerializer : public folly::io::Appender {
 public:
  using folly::io::Appender::Appender;
  void xdr_integer(int32_t value);
  void xdr_integer_unsigned(uint32_t value);
  void xdr_hyper_integer(int64_t value);
  void xdr_hyper_integer_unsigned(uint64_t value);
  void xdr_bool(bool value);
  void xdr_float(float value);
  void xdr_double(double value);

  // Serializes just the bytes with no length indicator;
  // the deserializer is assumed to know the size
  void xdr_opaque_fixed(folly::ByteRange bytes);

  // Serializes the bytes with a length indicator
  void xdr_opaque_variable(folly::ByteRange bytes);

  void xdr_string(folly::StringPiece str);

  // Round up to the XDR basic block size
  static inline size_t roundUp(size_t value) {
    return (value + 3) & ~3;
  }
};

// Code can assume that `void serializeXdr(XdrSerializer&, const T&)` is defined
// to a type T to serialize it to Xdr representation.
// This header provides some basic/generic implementations.
// It is expected that other code will define that function for structs
// that they define and wish to serialize.

void serializeXdr(XdrSerializer& xdr, folly::StringPiece value);
void serializeXdr(XdrSerializer& xdr, folly::ByteRange value);

template <size_t N>
void serializeXdr(XdrSerializer& xdr, const std::array<uint8_t, N>& array) {
  xdr.xdr_opaque_fixed(folly::ByteRange(array));
}

template <size_t N>
void serializeXdr(XdrSerializer& xdr, const std::vector<uint8_t>& array) {
  xdr.xdr_opaque_variable(folly::ByteRange(array));
}

template <class T>
typename std::enable_if<std::is_enum<T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  // Enums are serialized as signed integers
  xdr.xdr_integer(value);
}

template <class T>
typename std::enable_if<std::is_same<int32_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_integer(value);
}

template <class T>
typename std::enable_if<std::is_same<uint32_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_integer_unsigned(value);
}

template <class T>
typename std::enable_if<std::is_same<int64_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_hyper_integer(value);
}

template <class T>
typename std::enable_if<std::is_same<uint64_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_hyper_integer_unsigned(value);
}

template <class T>
typename std::enable_if<std::is_same<bool, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_bool(value);
}

template <class T>
typename std::enable_if<std::is_same<float, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_float(value);
}

template <class T>
typename std::enable_if<std::is_same<double, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    T value) {
  xdr.xdr_double(value);
}

// std::array is encoded as a fixed size array; there is no preceding
// length indicator
template <class T, size_t N>
typename std::enable_if<!std::is_same<uint8_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    const std::array<T, N>& array) {
  for (auto& item : array) {
    serializeXdr(xdr, item);
  }
}

// std::vector is encoded as a variable size array with a length indicator
template <class T>
typename std::enable_if<!std::is_same<uint8_t, T>::value>::type serializeXdr(
    XdrSerializer& xdr,
    const std::vector<T>& array) {
  xdr.xdr_integer_unsigned(array.size());
  for (auto& item : array) {
    serializeXdr(xdr, item);
  }
}

} // namespace facebook::eden
