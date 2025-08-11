/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/PrettyPrinters.h"

#include <folly/Conv.h>

namespace {
template <typename ThriftEnum>
void appendThriftEnum(
    const ThriftEnum& value,
    std::string* result,
    folly::StringPiece typeName) {
  const char* name = apache::thrift::TEnumTraits<ThriftEnum>::findName(value);
  if (name) {
    folly::toAppend(name, result);
  } else {
    folly::toAppend(typeName, "::", int(value), result);
  }
}
} // unnamed namespace

namespace facebook::eden {

void toAppend(const ConflictType& conflictType, std::string* result) {
  appendThriftEnum(conflictType, result, "ConflictType");
}

void toAppend(const CheckoutConflict& conflict, std::string* result) {
  folly::toAppend("CheckoutConflict(type=", result);
  appendThriftEnum(*conflict.type(), result, "ConflictType");
  folly::toAppend(", path=\"", result);
  folly::toAppend(*conflict.path(), result);
  folly::toAppend("\", message=\"", result);
  folly::toAppend(*conflict.message(), result);
  folly::toAppend("\")", result);
}

void toAppend(const ScmFileStatus& scmFileStatus, std::string* result) {
  appendThriftEnum(scmFileStatus, result, "ScmFileStatus");
}

void toAppend(const MountState& mountState, std::string* result) {
  appendThriftEnum(mountState, result, "MountState");
}

} // namespace facebook::eden
