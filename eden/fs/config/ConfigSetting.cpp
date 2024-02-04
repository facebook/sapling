/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/ConfigSetting.h"

namespace facebook::eden {

static const std::vector<ConfigSourceType> kConfigSourcesInPriorityOrder = {
    ConfigSourceType::CommandLine,
    ConfigSourceType::UserConfig,
    ConfigSourceType::Dynamic,
    ConfigSourceType::SystemConfig,
    ConfigSourceType::Default};

ConfigSettingBase::ConfigSettingBase(
    std::string_view key,
    const std::type_info& valueType,
    ConfigSettingManager* csm)
    : key_{key},
      valueType_{valueType},
      orderedConfigSources_{kConfigSourcesInPriorityOrder} {
  if (csm) {
    csm->registerConfiguration(this);
  }
}

} // namespace facebook::eden
