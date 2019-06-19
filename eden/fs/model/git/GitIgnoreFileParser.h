/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
