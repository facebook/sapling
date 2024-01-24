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

#include "eden/fs/utils/ChronoParse.h"
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
class FieldConverter<RelativePath> {
 public:
  /**
   * Convert the passed string piece to a repository RelativePath.
   * @param convData is a map of conversion data that can be used by conversions
   * method (for example $HOME value.)
   * @return the converted RelativePath or an error message.
   */
  folly::Expected<RelativePath, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const;

  std::string toDebugString(const RelativePath& path) const {
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
      assert(elements);
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

template <typename T>
class FieldConverter<std::unordered_set<T>> {
 public:
  folly::Expected<std::unordered_set<T>, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const {
    // TODO(xavierd): directly construct the set without the vector middle-step
    return FieldConverter<std::vector<T>>{}
        .fromString(value, convData)
        .then([](std::vector<T> vec) {
          return std::unordered_set<T>{
              std::make_move_iterator(vec.begin()),
              std::make_move_iterator(vec.end())};
        });
  }

  std::string toDebugString(const std::unordered_set<T>& value) const {
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

template <typename T>
class FieldConverter<std::shared_ptr<T>> {
 public:
  folly::Expected<std::shared_ptr<T>, std::string> fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const {
    return FieldConverter<T>{}.fromString(value, convData).then([](T val) {
      return std::make_shared<T>(std::move(val));
    });
  }

  std::string toDebugString(const std::shared_ptr<T>& value) const {
    return FieldConverter<T>{}.toDebugString(*value);
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

/**
 * A duration that can be constrained in a range.
 *
 * This can be used to prevent configs from being set too low or too high. Note
 * that due to C++ template limitation, the min and max times are expressed in
 * nanoseconds ticks.
 *
 * Throws a std::invalid_argument when constructed with a value out of range.
 * When used in a ConfigSettings, the old value will be preserved when trying
 * to set the config with an out of range value.
 */
template <
    int64_t MinNsTicks,
    int64_t MaxNsTicks = std::chrono::nanoseconds::max().count()>
struct ConstrainedDuration : public std::chrono::nanoseconds {
  template <class Rep, class Period>
  /* implicit */ ConstrainedDuration(std::chrono::duration<Rep, Period> time)
      : std::chrono::nanoseconds{time} {
    std::chrono::nanoseconds ns = time;
    if (ns.count() < MinNsTicks || ns.count() > MaxNsTicks) {
      throw std::invalid_argument(fmt::format(
          "Default Duration '{}' should be between {} and {}",
          durationToString(ns),
          durationToString(std::chrono::nanoseconds{MinNsTicks}),
          durationToString(std::chrono::nanoseconds{MaxNsTicks})));
    }
  }
};

constexpr int64_t OneHourTicks =
    std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::hours(1))
        .count();

using OneHourMinDuration = ConstrainedDuration<OneHourTicks>;

template <int64_t MinNsTicks, int64_t MaxNsTicks>
class FieldConverter<ConstrainedDuration<MinNsTicks, MaxNsTicks>>
    : private FieldConverter<std::chrono::nanoseconds> {
 public:
  folly::Expected<ConstrainedDuration<MinNsTicks, MaxNsTicks>, std::string>
  fromString(
      std::string_view value,
      const std::map<std::string, std::string>& convData) const {
    auto nanoseconds =
        FieldConverter<std::chrono::nanoseconds>{}.fromString(value, convData);
    return nanoseconds.then(
        [value](std::chrono::nanoseconds ns)
            -> folly::Expected<
                ConstrainedDuration<MinNsTicks, MaxNsTicks>,
                std::string> {
          if (ns.count() < MinNsTicks) {
            return folly::makeUnexpected<std::string>(fmt::format(
                "Value '{}' is smaller than the constraint ({})",
                value,
                durationToString(std::chrono::nanoseconds{MinNsTicks})));
          } else if (ns.count() > MaxNsTicks) {
            return folly::makeUnexpected<std::string>(fmt::format(
                "Value '{}' is bigger than the constraint ({})",
                value,
                durationToString(std::chrono::nanoseconds{MaxNsTicks})));
          } else {
            return ConstrainedDuration<MinNsTicks, MaxNsTicks>{ns};
          }
        });
  }

  std::string toDebugString(
      ConstrainedDuration<MinNsTicks, MaxNsTicks> value) const {
    return FieldConverter<std::chrono::nanoseconds>{}.toDebugString(value);
  }
};

} // namespace facebook::eden
