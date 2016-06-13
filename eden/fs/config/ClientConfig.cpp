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
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/json.h>

namespace {
// Keys in the config JSON.
constexpr folly::StringPiece kBindMountsKey{"bind-mounts"};
constexpr folly::StringPiece kMountKey{"mount"};
constexpr folly::StringPiece kRepoTypeKey{"repo_type"};
constexpr folly::StringPiece kRepoSourceKey{"repo_source"};

// Files of interest in the client directory.
const facebook::eden::RelativePathPiece kConfigFile{"config.json"};
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
  std::string snapshotFileContents;
  folly::readFile(snapshotFile.c_str(), snapshotFileContents);
  // Make sure to remove any leading or trailing whitespace.
  auto snapshotID = folly::trimWhitespace(snapshotFileContents);
  return Hash{snapshotID};
}

AbsolutePath ClientConfig::getOverlayPath() const {
  return clientDirectory_ + kOverlayDir;
}

std::unique_ptr<ClientConfig> ClientConfig::loadFromClientDirectory(
    AbsolutePathPiece clientDirectory) {
  // Extract the JSON and strip any comments.
  auto configJsonFile = clientDirectory + kConfigFile;
  std::string jsonContents;
  folly::readFile(configJsonFile.c_str(), jsonContents);
  auto jsonWithoutComments = folly::json::stripComments(jsonContents);

  // Parse the comment-free JSON while tolerating trailing commas.
  folly::json::serialization_opts options;
  options.allow_trailing_comma = true;
  auto configData = folly::parseJson(jsonWithoutComments, options);

  auto mountPoint = configData[kMountKey].asString();
  auto mountPointPath = AbsolutePath{mountPoint};

  // Extract the list of bind mounts.
  std::vector<BindMount> bindMounts;
  auto bindMountsJsonPtr = configData.get_ptr(kBindMountsKey);
  if (bindMountsJsonPtr != nullptr) {
    AbsolutePath bindMountsPath = clientDirectory + kBindMountsDir;
    for (auto& item : bindMountsJsonPtr->items()) {
      auto pathInClientDir =
          bindMountsPath + RelativePathPiece{item.first.asString()};
      auto pathInMountDir =
          mountPointPath + RelativePathPiece{item.second.asString()};
      bindMounts.emplace_back(BindMount{pathInClientDir, pathInMountDir});
    }
  }

  auto config = std::make_unique<ClientConfig>(
      ClientConfig(clientDirectory, mountPointPath, std::move(bindMounts)));

  // Load the repository information
  config->repoType_ = configData.getDefault(kRepoTypeKey, "null").asString();
  config->repoSource_ = configData.getDefault(kRepoSourceKey, "").asString();

  return config;
}
}
}
