/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

/*
 * This file contains helper functions related to parsing edenfs command line
 * arguments and determining the initial Eden configuration and state directory.
 *
 * This enables this logic to be shared by the main edenfs process as well as
 * other helper tools that need to be able to access the Eden state directory
 * and configuration data.
 */

#include <memory>
#include <string>

#include <folly/Conv.h>

#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class EdenConfig;
class UserInfo;

/**
 * Get the EdenConfig object.
 *
 * This processes the command line arguments and config settings to construct
 * the EdenConfig.  This also determines the location of the Eden state
 * directory, which can be obtained by calling EdenConfig::getEdenDir().
 * This function will create the Eden state directory on disk if it does not
 * already exist.
 */
std::unique_ptr<EdenConfig> getEdenConfig(UserInfo& identity);

/**
 * ArgumentError will be thrown by getEdenConfig() for common or expected
 * exceptions when trying to set up the Eden config data.  This includes issues
 * like bad command line arguments or errors creating or finding the expected
 * state and config data on disk.
 *
 * The caller of getEdenConfig() should generally catch ArgumentError exceptions
 * and display them nicely to the end user.
 */
class ArgumentError : public std::exception {
 public:
  template <class... Args>
  explicit ArgumentError(Args&&... args)
      : message_(folly::to<std::string>(std::forward<Args>(args)...)) {}

  const char* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

} // namespace eden
} // namespace facebook
