/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/ConfigSource.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class TomlFileConfigSource final : public ConfigSource {
 public:
  TomlFileConfigSource(AbsolutePath path, ConfigSourceType sourceType);

  ConfigSourceType getSourceType() override {
    return sourceType_;
  }

  std::string getSourcePath() override {
    return absolutePathToThrift(path_);
  }

  FileChangeReason shouldReload() override;

  void reload(const ConfigVariables& substitutions, ConfigSettingMap& map)
      override;

 private:
  void parseAndApply(
      int fd,
      const ConfigVariables& substitutions,
      ConfigSettingMap& map);

  AbsolutePath path_;
  ConfigSourceType sourceType_;
  std::optional<FileStat> lastStat_;
};

} // namespace facebook::eden
