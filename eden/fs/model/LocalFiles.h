/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

/**
 * Changes to local files from getScmStatus grouped by type.
 */
struct LocalFiles {
  std::unordered_set<std::string> addedFiles;
  std::unordered_set<std::string> removedFiles;
  std::unordered_set<std::string> modifiedFiles;
  std::unordered_set<std::string> ignoredFiles;
};

} // namespace facebook::eden
