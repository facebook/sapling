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

namespace facebook {
namespace eden {

/**
 * Pretty-print a CheckoutConflict
 */
std::ostream& operator<<(std::ostream& os, ConflictType conflictType) {
  auto iter = _ConflictType_VALUES_TO_NAMES.find(conflictType);
  if (iter == _ConflictType_VALUES_TO_NAMES.end()) {
    os << "ConflictType::" << int(conflictType);
  } else {
    os << iter->second;
  }
  return os;
}

/**
 * Pretty-print a CheckoutConflict
 */
std::ostream& operator<<(std::ostream& os, const CheckoutConflict& conflict) {
  os << "CheckoutConflict(type=" << conflict.type << ", path=\""
     << conflict.path << "\", message=\"" << conflict.message << "\")";
  return os;
}
}
}
