/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/xdr/XdrSerializer.h"

namespace facebook::eden {

void XdrSerializer::xdr_integer(int32_t value) {
  writeBE(value);
}

void XdrSerializer::xdr_integer_unsigned(uint32_t value) {
  writeBE(value);
}

void XdrSerializer::xdr_bool(bool value) {
  xdr_integer(value ? 1 : 0);
}

void XdrSerializer::xdr_hyper_integer(int64_t value) {
  writeBE(value);
}

void XdrSerializer::xdr_hyper_integer_unsigned(uint64_t value) {
  writeBE(value);
}

void XdrSerializer::xdr_float(float value) {
  writeBE(value);
}

void XdrSerializer::xdr_double(double value) {
  writeBE(value);
}

void XdrSerializer::xdr_opaque_fixed(folly::ByteRange bytes) {
  push(bytes);
  auto rounded = roundUp(bytes.size());
  for (size_t i = bytes.size(); i < rounded; ++i) {
    writeBE<uint8_t>(0);
  }
}

void XdrSerializer::xdr_opaque_variable(folly::ByteRange bytes) {
  xdr_integer_unsigned(bytes.size());
  xdr_opaque_fixed(bytes);
}

void XdrSerializer::xdr_string(folly::StringPiece str) {
  xdr_opaque_variable(folly::ByteRange(str));
}

void serializeXdr(XdrSerializer& xdr, folly::StringPiece value) {
  xdr.xdr_string(value);
}

void serializeXdr(XdrSerializer& xdr, folly::ByteRange value) {
  xdr.xdr_opaque_variable(value);
}

} // namespace facebook::eden
