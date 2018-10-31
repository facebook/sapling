/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/fs/config/EdenConfig.h"

namespace facebook {
namespace eden {

/** An interface that defines how to obtain a possibly reloaded EdenConfig
 * instance.
 *
 * This class is used to avoid passing down too much state information
 * from the top level of the server and into the depths.
 */
class ReloadableConfig {
 public:
  /**
   * Get the EdenConfig; We check for changes in the config files, reload as
   * necessary and return an updated EdenConfig. The update checks are
   * throttleSeconds to kEdenConfigMinPollSeconds. If 'skipUpdate' is set, no
   * update check is performed and the current EdenConfig is returned.
   */
  virtual std::shared_ptr<const EdenConfig> getEdenConfig(
      bool skipUpdate = false) = 0;
  virtual ~ReloadableConfig() {}
};

} // namespace eden
} // namespace facebook
