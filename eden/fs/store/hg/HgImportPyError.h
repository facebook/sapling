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
