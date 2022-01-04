/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
    static_cast<CaseSensitivity>(folly::kIsLinux);
} // namespace facebook::eden
