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

// Keys in the config JSON.
constexpr folly::StringPiece kBindMountsKey{"bind-mounts"};
constexpr folly::StringPiece kMountKey{"mount"};

namespace facebook {
namespace eden {

// Files of interest in the client directory.
const RelativePathPiece kConfigFile{"config.json"};
const RelativePathPiece kSnapshotFile{"SNAPSHOT"};
const RelativePathPiece kBindMountsDir{"bind-mounts"};

ClientConfig::ClientConfig(
    AbsolutePathPiece clientDirectory,
    AbsolutePathPiece mountPath,
    std::vector<BindMount>&& bindMounts)
    : clientDirectory_(clientDirectory),
      mountPath_(mountPath),
      bindMounts_(std::move(bindMounts)) {}

std::string ClientConfig::getSnapshotID() const {
  // Read the snapshot.
  auto snapshotFile = clientDirectory_ + kSnapshotFile;
  std::string snapshotFileContents;
  folly::readFile(snapshotFile.c_str(), snapshotFileContents);
  // Make sure to remove any leading or trailing whitespace.
  auto snapshotID = folly::trimWhitespace(snapshotFileContents);
  return snapshotID.str();
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

  return std::make_unique<ClientConfig>(
      ClientConfig(clientDirectory, mountPointPath, std::move(bindMounts)));
}
}
}
