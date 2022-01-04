/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

/**
 * If a return value, whether the caller is responsible for invalidating the
 * kernel's cache.
 *
 * If an argument, whether the callee is responsible for invalidating the
 * kernel's cache.
 */
enum class InvalidationRequired : bool {
  No,
  Yes,
};

} // namespace facebook::eden
