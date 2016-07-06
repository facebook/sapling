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
#include <boost/filesystem.hpp>
#include <boost/property_tree/ini_parser.hpp>
#include <boost/range/adaptor/reversed.hpp>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <pwd.h>
#include <stdlib.h>

using std::string;

namespace {
// INI config files
const facebook::eden::AbsolutePathPiece kGlobalConfig{"/etc/eden/config.d"};
const facebook::eden::RelativePathPiece kHomeConfig{".edenrc"};
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

std::unique_ptr<ClientConfig> ClientConfig::loadFromClientDirectory(
    AbsolutePathPiece mountPoint,
    AbsolutePathPiece clientDirectory,
    AbsolutePathPiece homeDirectory) {
  // Extract repo name from clientDirectory
  boost::property_tree::ptree repoData;
  auto clientConfigFile = clientDirectory + kLocalConfig;
  boost::property_tree::ini_parser::read_ini(
      clientConfigFile.c_str(), repoData);
  auto repo = repoData.get(kRepoNameKey.toString(), "");

  // Get global config files
  boost::filesystem::path rcDir(folly::to<string>(kGlobalConfig));
  std::vector<string> rcFiles;
  if (boost::filesystem::is_directory(rcDir)) {
    for (auto it : boost::filesystem::directory_iterator(rcDir)) {
      rcFiles.push_back(it.path().string());
    }
  }
  sort(rcFiles.begin(), rcFiles.end());

  // Get home config file
  auto userConfigFile = AbsolutePathPiece{homeDirectory} + kHomeConfig;
  rcFiles.push_back(userConfigFile.c_str());

  // Find repository data in config files
  boost::property_tree::ptree configData;
  string header = kRepositoryKey.toString() + repo;
  // Find the first config file that defines the [repository]
  for (auto rc : boost::adaptors::reverse(rcFiles)) {
    // Parse INI file into property tree
    boost::property_tree::ini_parser::read_ini(rc, configData);
    repoData = configData.get_child(
        header, boost::property_tree::basic_ptree<string, string>());
    if (!repoData.empty()) {
      break;
    }
  }
  // Repository not found
  if (repoData.empty()) {
    throw std::runtime_error("Could not find repository data for " + repo);
  }
  auto mountPointPath = AbsolutePath{mountPoint};

  // Extract the bind mounts
  std::vector<BindMount> bindMounts;
  header = kBindMountsKey.toString() + repo;
  auto bindMountPoints = configData.get_child(
      header, boost::property_tree::basic_ptree<string, string>());
  if (!bindMountPoints.empty()) {
    AbsolutePath bindMountsPath = clientDirectory + kBindMountsDir;
    for (auto item : bindMountPoints) {
      auto pathInClientDir = bindMountsPath + RelativePathPiece{item.first};
      auto pathInMountDir =
          mountPointPath + RelativePathPiece{item.second.data()};
      bindMounts.emplace_back(BindMount{pathInClientDir, pathInMountDir});
    }
  }

  // Construct ClientConfig object
  auto config = std::make_unique<ClientConfig>(
      ClientConfig(clientDirectory, mountPointPath, std::move(bindMounts)));

  // Load repository information
  config->repoType_ = repoData.get(kRepoTypeKey.toString(), "");
  config->repoSource_ = repoData.get(kRepoSourceKey.toString(), "");

  return config;
}
}
}
