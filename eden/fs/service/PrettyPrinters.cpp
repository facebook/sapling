/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/service/PrettyPrinters.h"

#include <folly/Conv.h>
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

template <typename ThriftEnum>
void appendThriftEnum(
    ThriftEnum value,
    std::string* result,
    const std::map<ThriftEnum, const char*>& valuesToNames,
    folly::StringPiece typeName) {
  auto iter = valuesToNames.find(value);
  if (iter == valuesToNames.end()) {
    result->append(folly::to<std::string>(typeName, "::", int(value)));
  } else {
    result->append(iter->second);
  }
}
} // unnamed namespace

namespace facebook {
namespace eden {

std::ostream& operator<<(std::ostream& os, ConflictType conflictType) {
  return outputThriftEnum(
      os, conflictType, _ConflictType_VALUES_TO_NAMES, "ConflictType");
}

std::ostream& operator<<(std::ostream& os, const CheckoutConflict& conflict) {
  os << "CheckoutConflict(type=" << conflict.type << ", path=\""
     << conflict.path << "\", message=\"" << conflict.message << "\")";
  return os;
}

std::ostream& operator<<(std::ostream& os, ScmFileStatus scmFileStatus) {
  return outputThriftEnum(
      os, scmFileStatus, _ScmFileStatus_VALUES_TO_NAMES, "ScmFileStatus");
}

std::ostream& operator<<(std::ostream& os, MountState mountState) {
  return outputThriftEnum(
      os, mountState, _MountState_VALUES_TO_NAMES, "MountState");
}

void toAppend(MountState mountState, std::string* result) {
  appendThriftEnum(
      mountState, result, _MountState_VALUES_TO_NAMES, "MountState");
}

} // namespace eden
} // namespace facebook
