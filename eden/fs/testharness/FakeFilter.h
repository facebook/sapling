/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/filter/Filter.h"

namespace facebook::eden {

/**
 * A BackingStore implementation for test code.
 */
class FakeFilter final : public Filter {
 public:
  ~FakeFilter() override {}

  /*
   * Checks whether a path is filtered by the given filter.
   */
  bool isPathFiltered(RelativePathPiece path, folly::StringPiece filterId)
      override {
    return path.view().find(filterId) != std::string::npos;
  }
};
} // namespace facebook::eden
