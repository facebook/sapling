/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
   * The environment variable name, or folly::none if this object has been
   * moved-away from.
   *
   * We could just use a std::string instead of std::optional<std::string>,
   * and used an empty string to indicate that this ScopedEnvVar has been
   * moved-away from.  However, then we would have to implement our own custom
   * move constructor and move assignment operator to clear the name in the
   * moved-from object.  With std::optional the moved-from value is
   * guaranteed to be cleared automatically, while this is not guaranteed with
   * std::string.
   */
  std::optional<std::string> name_;

  /**
   * The original value of this environment variable that we should restore it
   * to on destruction of this ScopedEnvVar.  This will be folly::none if the
   * environment variable was originally unset, and should be unset on
   * destruction.
   */
  std::optional<std::string> origValue_;
};
} // namespace eden
} // namespace facebook
