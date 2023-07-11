/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class Filter {
 public:
  virtual ~Filter() {}

  /*
   * Checks whether a path is filtered by the given filter.
   */
  virtual bool isPathFiltered(
      RelativePathPiece path,
      folly::StringPiece filterId) = 0;
};
} // namespace facebook::eden
