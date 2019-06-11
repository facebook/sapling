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
#include <type_traits>

#include <folly/Expected.h>
#include <folly/Range.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Converters are used to convert strings into ConfigSettings. For example,
 * they are used to convert the string settings of configuration files.
 */
template <typename T, typename Enable = void>
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
  folly::Expected<AbsolutePath, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(const AbsolutePath& path) const {
    return path.value();
  }
};

template <>
class FieldConverter<std::string> {
 public:
  folly::Expected<std::string, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(const std::string& value) const {
    return value;
  }
};

/*
 * FieldConverter implementation for integers, floating point, and bool types
 * using folly::to<T>(string)
 */
template <typename T>
class FieldConverter<
    T,
    typename std::enable_if<
        std::is_arithmetic<T>::value || std::is_same<T, bool>::value>::type> {
 public:
  /**
   * Convert the passed string piece to a boolean.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted boolean or an error message.
   */
  folly::Expected<T, std::string> fromString(
      folly::StringPiece value,
      const std::map<std::string, std::string>& /* convData */) const {
    auto result = folly::tryTo<T>(value);
    if (result.hasValue()) {
      return result.value();
    }
    return folly::makeUnexpected<std::string>(
        folly::makeConversionError(result.error(), value).what());
  }

  std::string toDebugString(T value) const {
    return folly::to<std::string>(value);
  }
};

} // namespace eden
} // namespace facebook
