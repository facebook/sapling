/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "ClientConfig.h"
#include <boost/algorithm/string.hpp>
#include <boost/filesystem.hpp>
#include <boost/range/adaptor/reversed.hpp>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <pwd.h>
#include <stdlib.h>

using std::string;

namespace {
// INI config file
const facebook::eden::RelativePathPiece kLocalConfig{"edenrc"};

// Keys for the config INI file.
constexpr folly::StringPiece kBindMountsKey{"bindmounts "};
constexpr folly::StringPiece kRepositoryKey{"repository "};
constexpr folly::StringPiece kRepoNameKey{"repository.name"};
constexpr folly::StringPiece kRepoTypeKey{"type"};
constexpr folly::StringPiece kRepoSourceKey{"path"};

// Files of interest in the client directory.
const facebook::eden::RelativePathPiece kSnapshotFile{"SNAPSHOT"};
const facebook::eden::RelativePathPiece kBindMountsDir{"bind-mounts"};
const facebook::eden::RelativePathPiece kOverlayDir{"local"};
}

namespace facebook {
namespace eden {

using defaultPtree = boost::property_tree::basic_ptree<string, string>;

ClientConfig::ClientConfig(
    AbsolutePathPiece clientDirectory,
    AbsolutePathPiece mountPath,
    std::vector<BindMount>&& bindMounts)
    : clientDirectory_(clientDirectory),
      mountPath_(mountPath),
      bindMounts_(std::move(bindMounts)) {}

Hash ClientConfig::getSnapshotID() const {
  // Read the snapshot.
  auto snapshotFile = clientDirectory_ + kSnapshotFile;
  string snapshotFileContents;
  folly::readFile(snapshotFile.c_str(), snapshotFileContents);
  // Make sure to remove any leading or trailing whitespace.
  auto snapshotID = folly::trimWhitespace(snapshotFileContents);
  return Hash{snapshotID};
}

AbsolutePath ClientConfig::getOverlayPath() const {
  return clientDirectory_ + kOverlayDir;
}

ClientConfig::ConfigData ClientConfig::loadConfigData(
    AbsolutePathPiece systemConfigDir,
    AbsolutePathPiece configPath) {
  ConfigData resultData;
  // Get global config files
  boost::filesystem::path rcDir(folly::to<string>(systemConfigDir));
  std::vector<string> rcFiles;
  if (boost::filesystem::is_directory(rcDir)) {
    for (auto it : boost::filesystem::directory_iterator(rcDir)) {
      rcFiles.push_back(it.path().string());
    }
  }
  sort(rcFiles.begin(), rcFiles.end());

  // Get home config file
  auto userConfigPath = AbsolutePath{configPath};
  rcFiles.push_back(userConfigPath.c_str());

  // Parse repository data in order to compile them
  for (auto rc : boost::adaptors::reverse(rcFiles)) {
    if (access(rc.c_str(), R_OK) != 0) {
      continue;
    }
    // Only add repository data from the first config file that references it
    ConfigData configData;
    boost::property_tree::ini_parser::read_ini(rc, configData);
    for (auto& entry : configData) {
      if (resultData.get_child(entry.first, defaultPtree()).empty()) {
        resultData.put_child(entry.first, entry.second);
      }
    }
  }
  return resultData;
}

std::unique_ptr<ClientConfig> ClientConfig::loadFromClientDirectory(
    AbsolutePathPiece mountPoint,
    AbsolutePathPiece clientDirectory,
    const ConfigData* configData) {
  // Extract repository name from the client config file
  ConfigData repoData;
  auto configFile = clientDirectory + kLocalConfig;
  boost::property_tree::ini_parser::read_ini(configFile.c_str(), repoData);
  auto repoName = repoData.get(kRepoNameKey.toString(), "");

  // Get the data of repository repoName from config files
  string repoHeader = kRepositoryKey.toString() + repoName;
  repoData = configData->get_child(repoHeader, defaultPtree());

  // Repository data not found
  if (repoData.empty()) {
    throw std::runtime_error("Could not find repository data for " + repoName);
  }

  // Extract the bind mounts
  std::vector<BindMount> bindMounts;
  string bindMountHeader = kBindMountsKey.toString() + repoName;
  auto bindMountPoints = configData->get_child(bindMountHeader, defaultPtree());

  AbsolutePath mountPath = AbsolutePath{mountPoint};
  AbsolutePath bindMountsPath = clientDirectory + kBindMountsDir;
  for (auto item : bindMountPoints) {
    auto pathInClientDir = bindMountsPath + RelativePathPiece{item.first};
    auto pathInMountDir = mountPath + RelativePathPiece{item.second.data()};
    bindMounts.emplace_back(BindMount{pathInClientDir, pathInMountDir});
  }

  // Construct ClientConfig object
  auto config = std::make_unique<ClientConfig>(
      ClientConfig(clientDirectory, mountPath, std::move(bindMounts)));

  // Load repository information
  config->repoType_ = repoData.get(kRepoTypeKey.toString(), "");
  config->repoSource_ = repoData.get(kRepoSourceKey.toString(), "");

  return config;
}
}
}
