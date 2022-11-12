/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <memory>
#include <optional>
#include <system_error>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

/**
 * A subclass of std::system_error referring to a specific path.
 *
 * The main advantage of this class is that it can include a path in
 * the error message.  However, it avoids computing the subclass's path until
 * the error message is actually needed.  If the error is caught and handled
 * without looking at the error message, then the path never needs to be
 * computed.
 */
class PathErrorBase : public std::system_error {
 public:
  explicit PathErrorBase(int errnum)
      : std::system_error(errnum, std::generic_category()) {}
  PathErrorBase(int errnum, std::string message)
      : std::system_error(errnum, std::generic_category()),
        message_(std::move(message)) {}
  ~PathErrorBase() override = default;

  const char* what() const noexcept override;

  PathErrorBase(PathErrorBase const&) = default;
  PathErrorBase& operator=(PathErrorBase const&) = default;
  PathErrorBase(PathErrorBase&&) = default;
  PathErrorBase& operator=(PathErrorBase&&) = default;

 protected:
  virtual std::string computePath() const noexcept = 0;

 private:
  std::string computeMessage() const;

  std::string message_;
  mutable folly::Synchronized<std::string> fullMessage_;
};

/**
 * A subclass of PathErrorBase referring to a specific path by string.
 *
 * Users should prefer InodeError to avoid copying and storing a string
 * unecesarily, but an inode isn't always available where PathErrorBase errors
 * are needed.
 */
class PathError : public PathErrorBase {
 public:
  explicit PathError(int errnum, RelativePathPiece path, std::string message)
      : PathErrorBase(errnum, std::move(message)), path_(path.copy()) {}
  explicit PathError(int errnum, RelativePathPiece path)
      : PathErrorBase(errnum), path_(path.copy()) {}
  ~PathError() override = default;

  PathError(PathError const&) = default;
  PathError& operator=(PathError const&) = default;
  PathError(PathError&&) = default;
  PathError& operator=(PathError&&) = default;

 protected:
  std::string computePath() const noexcept override {
    return path_.asString();
  }

 private:
  RelativePath path_;
};
} // namespace facebook::eden
