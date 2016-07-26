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
  boost::filesystem::path clientDir_;
  boost::filesystem::path mountPoint_;
  boost::filesystem::path userConfigPath_;

  virtual void SetUp() override {
    clientDir_ = boost::filesystem::temp_directory_path() /
        boost::filesystem::unique_path();
    boost::filesystem::create_directories(clientDir_);

    auto homeDir = boost::filesystem::temp_directory_path() /
        boost::filesystem::unique_path();
    boost::filesystem::create_directories(homeDir);

    mountPoint_ = "/tmp/someplace";

    auto snapshotPath = clientDir_ / "SNAPSHOT";
    auto snapshot = "1234567812345678123456781234567812345678\n";
    folly::writeFile(folly::StringPiece{snapshot}, snapshotPath.c_str());

    userConfigPath_ = homeDir / ".edenrc";
    auto data =
        "; This INI has a comment\n"
        "[repository fbsource]\n"
        "path = /data/users/carenthomas/fbsource\n"
        "type = git\n"
        "[bindmounts fbsource]\n"
        "my-path = path/to-my-path\n";
    folly::writeFile(folly::StringPiece{data}, userConfigPath_.c_str());

    auto localConfigPath = clientDir_ / "edenrc";
    auto localData =
        "[repository]\n"
        "name = fbsource\n";
    folly::writeFile(folly::StringPiece{localData}, localConfigPath.c_str());
  }

  virtual void TearDown() override {
    boost::filesystem::remove_all(clientDir_);
  }
};

TEST_F(ClientConfigTest, testLoadFromClientDirectory) {
  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()},
      AbsolutePath{clientDir_.string()},
      AbsolutePath{userConfigPath_.string()});

  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"},
      config->getSnapshotID());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  auto pathInClientDir = clientDir_ / "bind-mounts" / "my-path";

  expectedBindMounts.emplace_back(
      BindMount{AbsolutePath{pathInClientDir.c_str()},
                AbsolutePath{"/tmp/someplace/path/to-my-path"}});
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}

TEST_F(ClientConfigTest, testLoadFromClientDirectoryWithNoBindMounts) {
  // Overwrite .edenrc with no bind-mounts entry.
  auto data =
      "; This INI has a comment\n"
      "[repository fbsource]\n"
      "path = /data/users/carenthomas/fbsource\n"
      "type = git\n";
  folly::writeFile(folly::StringPiece{data}, userConfigPath_.c_str());

  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()},
      AbsolutePath{clientDir_.string()},
      AbsolutePath{userConfigPath_.string()});

  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"},
      config->getSnapshotID());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}
}
