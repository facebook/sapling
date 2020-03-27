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
    folly::StringPiece typeName) {
  const char* name = apache::thrift::TEnumTraits<ThriftEnum>::findName(value);
  if (name) {
    return os << name;
  } else {
    return os << typeName << "::" << int(value);
  }
}

template <typename ThriftEnum>
void appendThriftEnum(
    ThriftEnum value,
    std::string* result,
    folly::StringPiece typeName) {
  const char* name = apache::thrift::TEnumTraits<ThriftEnum>::findName(value);
  if (name) {
    result->append(name);
  } else {
    result->append(folly::to<std::string>(typeName, "::", int(value)));
  }
}
} // unnamed namespace

namespace facebook {
namespace eden {

std::ostream& operator<<(std::ostream& os, ConflictType conflictType) {
  return outputThriftEnum(os, conflictType, "ConflictType");
}

std::ostream& operator<<(std::ostream& os, const CheckoutConflict& conflict) {
  os << "CheckoutConflict(type=" << conflict.type << ", path=\""
     << conflict.path << "\", message=\"" << conflict.message << "\")";
  return os;
}

std::ostream& operator<<(std::ostream& os, ScmFileStatus scmFileStatus) {
  return outputThriftEnum(os, scmFileStatus, "ScmFileStatus");
}

std::ostream& operator<<(std::ostream& os, MountState mountState) {
  return outputThriftEnum(os, mountState, "MountState");
}

void toAppend(MountState mountState, std::string* result) {
  appendThriftEnum(mountState, result, "MountState");
}

} // namespace eden
} // namespace facebook
