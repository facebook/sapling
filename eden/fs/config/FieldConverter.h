/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <map>
#include <string>

#include <folly/Expected.h>
#include <folly/Range.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Converters are used to convert strings into ConfigSettings. For example,
 * they are used to convert the string settings of configuration files.
 */
template <typename T>
class FieldConverter {};

template <>
class FieldConverter<AbsolutePath> {
 public:
  /**
   * Convert the passed string piece to an AbsolutePath.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted AbsolutePath or an error message.
   */
  folly::Expected<AbsolutePath, std::string> operator()(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;
};

template <>
class FieldConverter<std::string> {
 public:
  folly::Expected<std::string, std::string> operator()(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;
};

template <>
class FieldConverter<bool> {
 public:
  /**
   * Convert the passed string piece to a boolean.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted boolean or an error message.
   */
  folly::Expected<bool, std::string> operator()(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;
};

template <>
class FieldConverter<uint16_t> {
 public:
  /**
   * Convert the passed string piece to a uint16_t.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted value or an error message.
   */
  folly::Expected<uint16_t, std::string> operator()(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;
};

} // namespace eden
} // namespace facebook
