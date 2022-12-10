/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cpptoml.h>
#include <fmt/ranges.h>
#include <folly/logging/xlog.h>
#include "eden/fs/utils/Throw.h"

namespace facebook::eden {

class TomlPath {
 public:
  /*implicit*/ constexpr TomlPath(std::initializer_list<std::string_view> list)
      : begin_{list.begin()}, end_{list.end()} {}

  constexpr TomlPath(const std::string_view* begin, const std::string_view* end)
      : begin_{begin}, end_{end} {}

  constexpr const std::string_view* begin() const {
    return begin_;
  }

  constexpr const std::string_view* end() const {
    return end_;
  }

  constexpr size_t size() const {
    return end_ - begin_;
  }

 private:
  const std::string_view* begin_;
  const std::string_view* end_;
};

/**
 * Given a root toml table, walks the table path given by key, and sets it to
 * defaultValue if not set.
 *
 * Returns a pair of the value at the given key (whether or not it was set) and
 * a boolean indicating whether the table was modified.
 *
 * Throws std::domain_error if the path through `root` specified by `key`
 * contains non-table values.
 */
template <typename T>
std::pair<T, bool>
setDefault(cpptoml::table& root, TomlPath key, const T& defaultValue) {
  // TODO: Much of this function could be moved into the .cpp file.
  XDCHECK_GE(key.size(), 1u);

  auto begin = key.begin();
  const auto end = key.end();

  cpptoml::table* table = &root;
  for (; begin + 1 < end; ++begin) {
    auto keystr = std::string{*begin};
    if (table->contains(keystr)) {
      auto entry = table->get(keystr);
      if (entry->is_table()) {
        table = static_cast<cpptoml::table*>(entry.get());
      } else {
        throwf<std::runtime_error>(
            "{} is not a table", fmt::join(key.begin(), begin + 1, "."));
      }
    } else {
      auto entry = cpptoml::make_table();
      auto newtable = entry.get();
      table->insert(keystr, std::move(entry));
      table = newtable;
    }
  }

  std::string keystr{*begin};
  if (table->contains(keystr)) {
    if (auto value = table->get(keystr)->as<T>()) {
      return std::make_pair(value->get(), false);
    } else {
      throwf<std::runtime_error>(
          "{} has mismatched type", fmt::join(key.begin(), key.end(), "."));
    }
  }
  table->insert(std::string{*begin}, defaultValue);
  return std::make_pair(defaultValue, true);
}

} // namespace facebook::eden
