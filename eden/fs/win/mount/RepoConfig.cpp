/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <cpptoml.h>
#include <iostream>
#include <memory>
#include <sstream>
#include "eden/fs/win/mount/RepoConfig.h"
#include "eden/fs/win/utils/FileUtils.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/WinError.h"

const std::string kConfigRootPath{"root"};
const std::string kConfigSocketPath{"socket"};
const std::string kConfigClientPath{"client"};
const std::string kConfigMountId{"mountid"};

const std::string kConfigTable{"Config"};

namespace facebook {
namespace eden {
void createRepoConfig(
    const AbsolutePath& repoPath,
    const AbsolutePath& socket,
    const AbsolutePath& client) {
  const auto dotEden{repoPath + ".eden"_pc};

  if (!CreateDirectoryA(dotEden.c_str(), nullptr)) {
    DWORD error = GetLastError();
    if (error != ERROR_ALREADY_EXISTS) {
      throw makeWin32ErrorExplicit(error, "Failed to create the .eden");
    }
  } else {
    // We should only write this config file once, otherwise it would overwrite
    // the mount id.
    const auto configFile{dotEden + "config"_pc};
    std::shared_ptr<cpptoml::table> rootTable = cpptoml::make_table();

    auto configTable = cpptoml::make_table();
    configTable->insert(kConfigRootPath, repoPath.c_str());
    configTable->insert(kConfigSocketPath, socket.c_str());
    configTable->insert(kConfigClientPath, client.c_str());
    configTable->insert(kConfigMountId, Guid::generate().toString());
    rootTable->insert(kConfigTable, configTable);

    std::stringstream stream;
    stream << (*rootTable);
    std::string contents = stream.str();
    writeFile(configFile.c_str(), contents);
  }
}

std::string getMountId(const std::string& repoPath) {
  std::string configPath{repoPath + "\\.eden\\config"};

  auto configRoot = cpptoml::parse_file(configPath);
  auto config = configRoot->get_table(kConfigTable);
  auto id = config->get_as<std::string>(kConfigMountId);
  if (!id) {
    throw std::logic_error("Mount id config missing");
  }
  return *id;
}
} // namespace eden
} // namespace facebook
