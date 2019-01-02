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

/*
 * This file contains functions for helping format some of the types defined
 * in eden.thrift.
 *
 * This is primarily useful for unit tests and logging.
 */

#include <iosfwd>
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook {
namespace eden {
std::ostream& operator<<(std::ostream& os, ConflictType conflictType);
std::ostream& operator<<(std::ostream& os, const CheckoutConflict& conflict);
std::ostream& operator<<(std::ostream& os, ScmFileStatus scmFileStatus);
std::ostream& operator<<(std::ostream& os, MountState mountState);

void toAppend(MountState conflictType, std::string* result);
} // namespace eden
} // namespace facebook
