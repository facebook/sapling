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
