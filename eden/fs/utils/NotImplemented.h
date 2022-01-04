/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "eden/fs/utils/EdenError.h"

namespace facebook {
namespace eden {

#define NOT_IMPLEMENTED()                               \
  do {                                                  \
    throw newEdenError(                                 \
        EdenErrorType::GENERIC_ERROR,                   \
        " +++++++  NOT IMPLEMENTED +++++++ Function: ", \
        __FUNCTION__,                                   \
        " Line: ",                                      \
        __LINE__);                                      \
  } while (true)

} // namespace eden
} // namespace facebook
