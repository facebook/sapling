/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Optional.h>
#include <folly/Range.h>
#include <string>
#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

/**
 * Convert a Hash to a std::string to be returned via thrift as a thrift
 * BinaryHash data type.
 */
inline std::string thriftHash(const Hash& hash) {
  return folly::StringPiece{hash.getBytes()}.str();
}

/**
 * Convert an folly::Optional<Hash> to a std::string to be returned via thrift
 * as a thrift BinaryHash data type.
 */
inline std::string thriftHash(const folly::Optional<Hash> hash) {
  if (hash.hasValue()) {
    return thriftHash(hash.value());
  }
  return std::string{};
}

/**
 * Convert thrift BinaryHash data type (a std::string containing the binary
 * hash bytes) into a Hash object.
 */
inline Hash hashFromThrift(const std::string& commitID) {
  return Hash(folly::ByteRange(folly::StringPiece(commitID)));
}
}
}
