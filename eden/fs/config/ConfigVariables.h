/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <map>
#include <string>

namespace facebook::eden {

class UserInfo;

/**
 * Our configs support variable substitution.
 *
 * This struct centralizes the construction of the variable substitution map.
 */
class ConfigVariables : public std::map<std::string, std::string> {
 public:
  ConfigVariables() = default;
  ConfigVariables(ConfigVariables&&) = default;
  ConfigVariables& operator=(ConfigVariables&&) = default;
};

ConfigVariables getUserConfigVariables(const UserInfo& userInfo);

} // namespace facebook::eden
