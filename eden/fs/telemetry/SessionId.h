/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>

namespace facebook::eden {

/**
 * Returns a random, process-stable positive integer in the range of [0,
 * UINT32_MAX]
 */
uint32_t getSessionId();

} // namespace facebook::eden
