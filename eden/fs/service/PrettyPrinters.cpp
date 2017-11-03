/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/PrettyPrinters.h"

#include <ostream>

namespace {
template <typename ThriftEnum>
std::ostream& outputThriftEnum(
    std::ostream& os,
    ThriftEnum value,
    const std::map<ThriftEnum, const char*>& valuesToNames,
    folly::StringPiece typeName) {
  auto iter = valuesToNames.find(value);
  if (iter == valuesToNames.end()) {
    os << typeName << "::" << int(value);
  } else {
    os << iter->second;
  }
  return os;
}
} // unnamed namespace

namespace facebook {
namespace eden {

/**
 * Pretty-print a CheckoutConflict
 */
std::ostream& operator<<(std::ostream& os, ConflictType conflictType) {
  return outputThriftEnum(
      os, conflictType, _ConflictType_VALUES_TO_NAMES, "ConflictType");
}

/**
 * Pretty-print a CheckoutConflict
 */
std::ostream& operator<<(std::ostream& os, const CheckoutConflict& conflict) {
  os << "CheckoutConflict(type=" << conflict.type << ", path=\""
     << conflict.path << "\", message=\"" << conflict.message << "\")";
  return os;
}

/**
 * Pretty-print a StatusCode
 */
std::ostream& operator<<(std::ostream& os, StatusCode statusCode) {
  return outputThriftEnum(
      os, statusCode, _StatusCode_VALUES_TO_NAMES, "StatusCode");
}
} // namespace eden
} // namespace facebook
