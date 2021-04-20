/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>

namespace facebook::eden {
enum class CaseSensitivity : bool {
  Insensitive = false,
  Sensitive = true,
};

constexpr CaseSensitivity kPathMapDefaultCaseSensitive =
    static_cast<CaseSensitivity>(!folly::kIsWindows);
} // namespace facebook::eden
