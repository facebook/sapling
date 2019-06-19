/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Range.h>
#include <stdexcept>
#include <string>

namespace facebook {
namespace eden {

/**
 * All exceptions received from the python hg_import_helper.py script
 * are thrown in C++ as HgImportPyError exceptions.
 */
class HgImportPyError : public std::exception {
 public:
  HgImportPyError(folly::StringPiece errorType, folly::StringPiece message);

  const char* what() const noexcept override {
    return fullMessage_.c_str();
  }

  /**
   * The name of the python exception type.
   */
  folly::StringPiece errorType() const noexcept {
    return errorType_;
  }

  /**
   * The python exception message.
   */
  folly::StringPiece message() const noexcept {
    return message_;
  }

 private:
  static constexpr folly::StringPiece kSeparator{": "};

  /**
   * The full message to return from what().
   * This always has the form "errorType: message"
   */
  const std::string fullMessage_;

  /**
   * The name of the python exception type.
   * This points to a substring of fullMessage_.
   */
  const folly::StringPiece errorType_;

  /**
   * The python exception message.
   * This points to a substring of fullMessage_.
   */
  const folly::StringPiece message_;
};

} // namespace eden
} // namespace facebook
