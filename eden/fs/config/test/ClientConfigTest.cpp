/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <boost/filesystem.hpp>
#include <folly/FileUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::BindMount;
using facebook::eden::ClientConfig;
using facebook::eden::Hash;
using facebook::eden::RelativePath;

namespace {

class ClientConfigTest : public ::testing::Test {
 protected:
  boost::filesystem::path configDir_;

  virtual void SetUp() {
    configDir_ = boost::filesystem::temp_directory_path() /
        boost::filesystem::unique_path();
    boost::filesystem::create_directories(configDir_);

    auto snapshotPath = configDir_ / "SNAPSHOT";
    auto snapshot = "1234567812345678123456781234567812345678\n";
    folly::writeFile(folly::StringPiece{snapshot}, snapshotPath.c_str());

    auto configPath = configDir_ / "config.json";
    auto data =
        "/* This JSON has a comment and a trailing comma */\n"
        "{\n"
        "  \"bind-mounts\": {\n"
        "    \"my-path\": \"path/to-my-path\""
        "  },\n"
        "  \"mount\": \"/tmp/someplace\",\n"
        "}";
    folly::writeFile(folly::StringPiece{data}, configPath.c_str());
  }

  virtual void TearDown() {
    boost::filesystem::remove_all(configDir_);
  }
};

TEST_F(ClientConfigTest, testLoadFromClientDirectory) {
  auto config =
      ClientConfig::loadFromClientDirectory(AbsolutePath{configDir_.string()});

  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"},
      config->getSnapshotID());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  auto pathInClientDir = configDir_ / "bind-mounts" / "my-path";

  expectedBindMounts.emplace_back(
      BindMount{AbsolutePath{pathInClientDir.c_str()},
                AbsolutePath{"/tmp/someplace/path/to-my-path"}});
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}

TEST_F(ClientConfigTest, testLoadFromClientDirectoryWithNoBindMounts) {
  // Overwrite config.json with no bind-mounts entry.
  auto configPath = configDir_ / "config.json";
  auto data =
      "{\n"
      "  \"mount\": \"/tmp/someplace\""
      "}";
  folly::writeFile(folly::StringPiece{data}, configPath.c_str());

  auto config =
      ClientConfig::loadFromClientDirectory(AbsolutePath{configDir_.string()});

  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"},
      config->getSnapshotID());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}
}
