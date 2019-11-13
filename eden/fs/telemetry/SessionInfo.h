/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>

namespace facebook {
namespace eden {

struct SessionInfo {
  std::string username;
  std::string hostname;
  std::string os;
  std::string osVersion;
  std::string edenVersion;
};

std::string getOperatingSystemName();
std::string getOperatingSystemVersion();

/**
 * Returns the result of calling gethostname() in a std::string. Throws an
 * exception on failure.
 */
std::string getHostname();

} // namespace eden
} // namespace facebook
