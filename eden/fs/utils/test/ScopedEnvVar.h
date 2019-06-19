/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/Range.h>
#include <optional>

namespace facebook {
namespace eden {

/**
 * A helper class for manipulating an environment variable,
 * and restoring it to its original state at the end of the current scope.
 */
class ScopedEnvVar {
 public:
  explicit ScopedEnvVar(folly::StringPiece name);
  ~ScopedEnvVar();

  /**
   * Unset the environment variable.
   */
  void unset();

  /**
   * Set the environment variable
   */
  void set(const char* value);
  void set(const std::string& value);
  void set(folly::StringPiece value);

 private:
  /**
   * The environment variable name.
   */
  std::optional<std::string> name_;

  /**
   * The original value of this environment variable that we should restore it
   * to on destruction of this ScopedEnvVar.  This will be std::nullopt if the
   * environment variable was originally unset, and should be unset on
   * destruction.
   */
  std::optional<std::string> origValue_;
};
} // namespace eden
} // namespace facebook
