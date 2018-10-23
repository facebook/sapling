/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <stdint.h>

namespace facebook {
namespace eden {

/**
 * Very efficiently returns a new uint64_t unique to this process. Amortizes
 * the cost of synchronizing threads across many ID allocations.
 *
 * TODO: It could be beneficial to add a parameter to request more than one
 * unique ID at a time.
 */
uint64_t generateUniqueID();

} // namespace eden
} // namespace facebook
