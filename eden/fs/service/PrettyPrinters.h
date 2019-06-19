/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
