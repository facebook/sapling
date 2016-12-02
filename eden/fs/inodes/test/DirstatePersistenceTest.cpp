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
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/inodes/DirstatePersistence.h"
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"

using namespace facebook::eden;
using apache::thrift::CompactSerializer;
using folly::test::TemporaryFile;

TEST(DirstatePersistence, saveAndReadDirectivesBackOut) {
  TemporaryFile storageFile("eden_test");

  AbsolutePath storageFilePath(storageFile.path().c_str());
  DirstatePersistence persistence(storageFilePath);
  std::unordered_map<RelativePath, overlay::UserStatusDirective>
      userDirectives = {
          {RelativePath("add.txt"), overlay::UserStatusDirective::Add},
          {RelativePath("remove.txt"), overlay::UserStatusDirective::Remove},
      };
  persistence.save(userDirectives);

  auto rehydratedDirectives = persistence.load();
  EXPECT_EQ(userDirectives, rehydratedDirectives);
}

TEST(DirstatePersistence, loadFromFileWithWellFormattedData) {
  TemporaryFile storageFile("eden_test");

  overlay::DirstateData dirstateData;
  dirstateData.directives = {
      {"add.txt", overlay::UserStatusDirective::Add},
      {"remove.txt", overlay::UserStatusDirective::Remove}};
  auto serializedData = CompactSerializer::serialize<std::string>(dirstateData);
  folly::writeFull(
      storageFile.fd(), serializedData.data(), serializedData.size());

  AbsolutePath storageFilePath(storageFile.path().c_str());
  DirstatePersistence persistence(storageFilePath);
  auto directives = persistence.load();
  std::unordered_map<RelativePath, overlay::UserStatusDirective>
      expectedDirectives = {
          {RelativePath("add.txt"), overlay::UserStatusDirective::Add},
          {RelativePath("remove.txt"), overlay::UserStatusDirective::Remove},
      };
  EXPECT_EQ(expectedDirectives, directives);
}

TEST(DirstatePersistence, attemptLoadFromNonExistentFile) {
  AbsolutePath storageFilePath;
  {
    TemporaryFile storageFile("eden_test");
    storageFilePath = AbsolutePath(storageFile.path().c_str());
  }
  DirstatePersistence persistence(storageFilePath);
  auto directives = persistence.load();
  EXPECT_EQ(0, directives.size());
}
