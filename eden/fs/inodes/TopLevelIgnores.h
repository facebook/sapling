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

#include "eden/fs/model/git/GitIgnore.h"
#include "eden/fs/model/git/GitIgnoreStack.h"

namespace facebook {
namespace eden {

class UserInfo;

/**
 * Encapsulate the system and user ignore files loaded from configuration files.
 * They are created by ServerState and used to populate the DiffState.
 */
class TopLevelIgnores {
 public:
  /**
   * Construct from provided user and system GitIgnore objects.
   */
  TopLevelIgnores(GitIgnore userIgnore, GitIgnore systemIgnore)
      : systemIgnoreStack_{nullptr, systemIgnore},
        userIgnoreStack_{&systemIgnoreStack_, userIgnore} {}
  /**
   * Construct from user and system gitIgnore file contents.
   * Intended for testing purposes.
   */
  TopLevelIgnores(
      folly::StringPiece systemIgnoreFileContents,
      folly::StringPiece userIgnoreFileContents)
      : systemIgnoreStack_{nullptr, systemIgnoreFileContents},
        userIgnoreStack_{&systemIgnoreStack_, userIgnoreFileContents} {}

  /**
   * Construct by parsing the user and system ignore files.
   * TODO: Remove these once ServerState changes with TopLevelIgnores land
   */
  explicit TopLevelIgnores(const UserInfo& userInfo);

  TopLevelIgnores(const TopLevelIgnores&) = delete;
  TopLevelIgnores(TopLevelIgnores&&) = delete;
  TopLevelIgnores& operator=(const TopLevelIgnores&) = delete;
  TopLevelIgnores& operator=(TopLevelIgnores&&) = delete;
  ~TopLevelIgnores() {}
  const GitIgnoreStack* getStack() const {
    if (!userIgnoreStack_.empty()) {
      return &userIgnoreStack_;
    }
    if (!systemIgnoreStack_.empty()) {
      return &systemIgnoreStack_;
    }
    return nullptr;
  }

 private:
  // TODO: Remove these once ServerState changes with TopLevelIgnores land
  std::string tryIngestFile(AbsolutePathPiece fileName);
  AbsolutePath constructUserIgnoreFileName(const UserInfo& userInfo);

  static constexpr folly::StringPiece kSystemWideIgnoreFileName =
      "/etc/eden/ignore";
  GitIgnoreStack systemIgnoreStack_;
  GitIgnoreStack userIgnoreStack_;
};
} // namespace eden
} // namespace facebook
