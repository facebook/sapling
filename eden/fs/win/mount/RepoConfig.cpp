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
#include "eden/fs/win/utils/WinError.h"

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
  }

  const auto configFile{dotEden + "config"_pc};
  std::shared_ptr<cpptoml::table> rootTable = cpptoml::make_table();

  auto configTable = cpptoml::make_table();
  configTable->insert("root", repoPath.c_str());
  configTable->insert("socket", socket.c_str());
  configTable->insert("client", client.c_str());
  rootTable->insert("Config", configTable);

  std::stringstream stream;
  stream << (*rootTable);
  std::string contents = stream.str();
  writeFile(configFile.c_str(), contents);
}
} // namespace eden
} // namespace facebook
