/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/xdr/XdrDeSerializer.h"
#include "eden/fs/nfs/xdr/XdrSerializer.h"

namespace facebook::eden {

int32_t XdrDeSerializer::xdr_integer() {
  return readBE<int32_t>();
}

uint32_t XdrDeSerializer::xdr_integer_unsigned() {
  return readBE<uint32_t>();
}

int64_t XdrDeSerializer::xdr_hyper_integer() {
  return readBE<int64_t>();
}

uint64_t XdrDeSerializer::xdr_hyper_integer_unsigned() {
  return readBE<uint64_t>();
}

bool XdrDeSerializer::xdr_bool() {
  return xdr_integer() ? true : false;
}

float XdrDeSerializer::xdr_float() {
  return readBE<float>();
}

double XdrDeSerializer::xdr_double() {
  return readBE<double>();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, int32_t& value) {
  value = xdr.xdr_integer();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, uint32_t& value) {
  value = xdr.xdr_integer_unsigned();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, int64_t& value) {
  value = xdr.xdr_hyper_integer();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, uint64_t& value) {
  value = xdr.xdr_hyper_integer_unsigned();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, bool& value) {
  value = xdr.xdr_bool();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, float& value) {
  value = xdr.xdr_float();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, double& value) {
  value = xdr.xdr_double();
}

void deSerializeXdrInto(XdrDeSerializer& xdr, std::string& result) {
  auto len = xdr.xdr_integer_unsigned();
  result = xdr.readFixedString(len);
  auto rounded = XdrSerializer::roundUp(len);
  // Variable sized buffers are 4-bytes aligned, make sure to skip these.
  xdr.skip(rounded - len);
}

void deSerializeXdrInto(XdrDeSerializer& xdr, std::vector<uint8_t>& result) {
  auto len = xdr.xdr_integer_unsigned();
  result.resize(len);
  xdr.pull(result.data(), len);
  auto rounded = XdrSerializer::roundUp(len);
  // Variable sized buffers are 4-bytes aligned, make sure to skip these.
  xdr.skip(rounded - len);
}

} // namespace facebook::eden
