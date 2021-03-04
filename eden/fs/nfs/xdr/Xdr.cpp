/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/xdr/Xdr.h"

namespace facebook::eden {

namespace {
void addPadding(folly::io::Appender& appender, size_t len) {
  auto paddingBytes = detail::roundUp(len) - len;
  for (size_t i = 0; i < paddingBytes; i++) {
    appender.writeBE<uint8_t>(0);
  }
}
} // namespace

namespace detail {
void serialize_fixed(folly::io::Appender& appender, folly::ByteRange value) {
  appender.push(value);
  addPadding(appender, value.size());
}

void serialize_variable(folly::io::Appender& appender, folly::ByteRange value) {
  XdrTrait<uint32_t>::serialize(appender, value.size());
  serialize_fixed(appender, value);
}

void serialize_iobuf(folly::io::Appender& appender, const folly::IOBuf& buf) {
  auto len = buf.computeChainDataLength();
  if (len > std::numeric_limits<uint32_t>::max()) {
    throw std::length_error(
        "XDR cannot encode variable sized array bigger than 4GB");
  }
  XdrTrait<uint32_t>::serialize(appender, folly::to_narrow(len));
  appender.push(folly::io::Cursor(&buf), len);
  addPadding(appender, len);
}

} // namespace detail

} // namespace facebook::eden

#endif
