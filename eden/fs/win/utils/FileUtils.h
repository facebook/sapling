/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/Handle.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/Range.h"
#include "folly/portability/IOVec.h"

namespace facebook {
namespace eden {

Hash getFileSha1(AbsolutePathPiece filePath);

} // namespace eden
} // namespace facebook
