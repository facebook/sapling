/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include "eden/fs/config/ClientConfig.h"
#include "eden/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::BindMount;
using facebook::eden::ClientConfig;
using facebook::eden::Hash;
using facebook::eden::RelativePath;

namespace {

using folly::test::TemporaryDirectory;
using TemporaryDirectory::Scope::PERMANENT;
using folly::test::TemporaryFile;

class ClientConfigTest : public ::testing::Test {
 protected:
  std::unique_ptr<TemporaryDirectory> edenDir_;
  folly::fs::path clientDir_;
  folly::fs::path systemConfigDir_;
  folly::fs::path mountPoint_;
  folly::fs::path userConfigPath_;

  virtual void SetUp() override {
    edenDir_ = std::make_unique<TemporaryDirectory>(
        "eden_config_test_", "", PERMANENT);

    clientDir_ = edenDir_->path() / "client";
    folly::fs::create_directory(clientDir_);
    systemConfigDir_ = edenDir_->path() / "config.d";
    folly::fs::create_directory(systemConfigDir_);
    mountPoint_ = "/tmp/someplace";

    auto snapshotPath = clientDir_ / "SNAPSHOT";
    auto snapshot = "1234567812345678123456781234567812345678\n";
    folly::writeFile(folly::StringPiece{snapshot}, snapshotPath.c_str());

    userConfigPath_ = edenDir_->path() / ".edenrc";
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
    edenDir_.reset();
  }
};

TEST_F(ClientConfigTest, testLoadFromClientDirectory) {
  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()},
      AbsolutePath{clientDir_.string()},
      AbsolutePath{systemConfigDir_.string()},
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
      AbsolutePath{systemConfigDir_.string()},
      AbsolutePath{userConfigPath_.string()});

  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"},
      config->getSnapshotID());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}

TEST_F(ClientConfigTest, testOverrideSystemConfigData) {
  auto systemConfigPath = systemConfigDir_ / "config.d";
  auto data =
      "; This INI has a comment\n"
      "[repository fbsource]\n"
      "path = /data/users/carenthomas/linux\n"
      "type = git\n"
      "[bindmounts fbsource]\n"
      "my-path = path/to-my-path\n";
  folly::writeFile(folly::StringPiece{data}, systemConfigPath.c_str());

  data =
      "; This INI has a comment\n"
      "[repository fbsource]\n"
      "path = /data/users/carenthomas/fbsource\n"
      "type = git\n";
  folly::writeFile(folly::StringPiece{data}, userConfigPath_.c_str());

  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()},
      AbsolutePath{clientDir_.string()},
      AbsolutePath{systemConfigDir_.string()},
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

TEST_F(ClientConfigTest, testOnlySystemConfigData) {
  auto systemConfigPath = systemConfigDir_ / "config.d";
  auto data =
      "; This INI has a comment\n"
      "[repository fbsource]\n"
      "path = /data/users/carenthomas/linux\n"
      "type = git\n"
      "[bindmounts fbsource]\n"
      "my-path = path/to-my-path\n";
  folly::writeFile(folly::StringPiece{data}, systemConfigPath.c_str());

  folly::writeFile(folly::StringPiece{""}, userConfigPath_.c_str());

  auto config = ClientConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()},
      AbsolutePath{clientDir_.string()},
      AbsolutePath{systemConfigDir_.string()},
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
}
