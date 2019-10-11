/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <stdint.h>

namespace facebook {
namespace eden {

/**
 * Very efficiently returns a new uint64_t unique to this process. Amortizes
 * the cost of synchronizing threads across many ID allocations.
 *
 * All returned IDs are nonzero.
 *
 * TODO: It might be beneficial to add a parameter to request more than one
 * unique ID at a time, though such an API would make it possible to exhaust
 * the range of a 64-bit integer.
 */
uint64_t generateUniqueID() noexcept;

} // namespace eden
} // namespace facebook
