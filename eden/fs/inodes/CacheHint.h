/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

namespace facebook {
namespace eden {

/**
 * A hint to read or fetch APIs about whether they should internally cache or
 * not.
 */
enum class CacheHint {
  /**
   * The result of this fetch will be cached by the caller and thus does not
   * need to be cached internally.
   */
  NotNeededAgain,

  /**
   * The result of this read or fetch will likely be requested again, so it
   * should be cached if possible.
   */
  LikelyNeededAgain,
};

} // namespace eden
} // namespace facebook
