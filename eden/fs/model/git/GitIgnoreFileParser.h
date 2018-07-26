/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Expected.h>
#include "eden/fs/model/git/GitIgnore.h"

namespace facebook {
namespace eden {

/**
 * GitIgnoreFileParser will parse a file and construct a GitIgnore from its
 * contents.
 * @see CachedParsedFileMonitor for usage details.
 */
class GitIgnoreFileParser {
 public:
  using value_type = GitIgnore;
  /**
   * Parse file and construct a GitIgnore from its contents.
   * @return the GitIgnore on success or non-zero error code on failure.
   */
  folly::Expected<GitIgnore, int> operator()(
      int fileDescriptor,
      AbsolutePathPiece filePath) const;
};
} // namespace eden
} // namespace facebook
