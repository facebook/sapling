/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

namespace cpp2 facebook.eden
namespace java com.facebook.eden.thrift
namespace py facebook.eden.eden_config

/**
 * ConfigSource identifies the point of origin of a config setting.
 * It is ordered from low to high precedence. Higher precedence
 * configuration values over-ride lower precedence values. A config
 * setting of CommandLine takes precedence over all other settings.
 */
enum ConfigSource {
  Default = 0
  SystemConfig = 1
  UserConfig = 2
  CommandLine = 3
}
