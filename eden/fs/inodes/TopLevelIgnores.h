/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/model/git/GitIgnore.h"
#include "eden/fs/model/git/GitIgnoreStack.h"

namespace facebook {
namespace eden {

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
  GitIgnoreStack systemIgnoreStack_;
  GitIgnoreStack userIgnoreStack_;
};
} // namespace eden
} // namespace facebook
