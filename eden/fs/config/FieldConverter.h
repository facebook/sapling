/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <map>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <type_traits>

#include <cpptoml.h>
#include <fmt/ranges.h>
#include <re2/re2.h>

#include <folly/Expected.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

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
      std::string_view value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(const AbsolutePath& path) const {
    return path.value();
  }
};

template <>
class FieldConverter<std::string> {
 public:
  folly::Expected<std::string, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(const std::string& value) const {
    return value;
  }
};

template <typename T>
class FieldConverter<std::optional<T>> {
 public:
  folly::Expected<std::optional<T>, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const {
    return FieldConverter<T>{}.fromString(value, convData);
  }

  std::string toDebugString(const std::optional<T>& value) const {
    return FieldConverter<T>{}.toDebugString(value.value_or(T{}));
  }
};

template <typename T>
class FieldConverter<std::vector<T>> {
 public:
  folly::Expected<std::vector<T>, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const {
    // make the array parsable by cpptoml
    std::string kArrayKeyName{"array"};
    std::istringstream valueStream{
        fmt::format("{} = {}", kArrayKeyName, value)};

    // parse in toml type
    std::shared_ptr<cpptoml::array> elements;
    try {
      cpptoml::parser parser{valueStream};
      auto table = parser.parse();
      elements = table->get_array(kArrayKeyName);
    } catch (cpptoml::parse_exception& err) {
      return folly::Unexpected(
          fmt::format("Error parsing an array of strings: {}", err.what()));
    }

    // parse from toml type to eden type
    std::vector<T> deserializedElements;
    deserializedElements.reserve(elements->get().size());
    for (auto& element : *elements) {
      auto stringElement = cpptoml::get_impl<std::string>(element);
      if (stringElement) {
        auto deserializedElement =
            FieldConverter<T>{}.fromString(*stringElement, convData);
        if (deserializedElement.hasValue()) {
          deserializedElements.push_back(deserializedElement.value());
        } else {
          return folly::Unexpected(deserializedElement.error());
        }
      } else {
        return folly::Unexpected<std::string>(
            "eden currenly only supports lists of strings for config values");
      }
    }
    return deserializedElements;
  }

  std::string toDebugString(const std::vector<T>& value) const {
    std::vector<std::string> serializedElements;
    serializedElements.resize(value.size());
    std::transform(
        value.begin(),
        value.end(),
        serializedElements.begin(),
        [](auto& element) {
          return FieldConverter<T>{}.toDebugString(element);
        });
    return fmt::to_string(fmt::join(serializedElements, ", "));
  }
};

/*
 * FieldConverter implementation for integers, floating point, and bool types
 */
template <typename T>
class FieldConverter<
    T,
    typename std::enable_if<std::is_arithmetic<T>::value>::type> {
 public:
  /**
   * Convert the passed string piece to a boolean.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted boolean or an error message.
   */
  folly::Expected<T, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& /* convData */) const {
    auto result = folly::tryTo<T>(value);
    if (result.hasValue()) {
      return result.value();
    }
    return folly::makeUnexpected<std::string>(
        folly::makeConversionError(result.error(), value).what());
  }

  std::string toDebugString(T value) const {
    if constexpr (std::is_same<T, bool>::value) {
      return value ? "true" : "false";
    }
    return folly::to<std::string>(value);
  }
};

/*
 * FieldConverter implementation for nanoseconds.
 *
 * We could fairly easily implement this for other duration types, but we would
 * have to decide what to do if the config specifies a more granular input
 * value.  e.g., if we wanted to parse a config field as `std::chrono::minutes`
 * what should we do if the value in the config file was "10s"?
 */
template <>
class FieldConverter<std::chrono::nanoseconds> {
 public:
  folly::Expected<std::chrono::nanoseconds, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(std::chrono::nanoseconds value) const;
};

template <>
class FieldConverter<std::shared_ptr<re2::RE2>> {
 public:
  folly::Expected<std::shared_ptr<re2::RE2>, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(std::shared_ptr<re2::RE2> value) const;
};

} // namespace facebook::eden
