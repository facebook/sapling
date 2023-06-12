/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/String.h"

namespace facebook::eden {

std::vector<std::string_view> split(std::string_view s, char delim) {
  std::vector<std::string_view> result;
  std::size_t i = 0;

  while ((i = s.find(delim)) != std::string_view::npos) {
    result.emplace_back(s.substr(0, i));
    s.remove_prefix(i + 1);
  }
  result.emplace_back(s);
  return result;
}

} // namespace facebook::eden
