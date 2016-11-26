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
#include "eden/fs/model/hg/LocalDirstatePersistence.h"
#include "eden/fs/model/hg/gen-cpp2/dirstate_types.h"

using namespace facebook::eden;
using apache::thrift::CompactSerializer;
using folly::test::TemporaryFile;

TEST(LocalDirstatePersistence, saveAndReadDirectivesBackOut) {
  TemporaryFile storageFile;

  AbsolutePath storageFilePath(storageFile.path().c_str());
  LocalDirstatePersistence persistence(storageFilePath);
  std::unordered_map<RelativePath, HgUserStatusDirective> userDirectives = {
      {RelativePath("add.txt"), HgUserStatusDirective::ADD},
      {RelativePath("remove.txt"), HgUserStatusDirective::REMOVE},
  };
  persistence.save(userDirectives);

  auto rehydratedDirectives = persistence.load();
  EXPECT_EQ(userDirectives, rehydratedDirectives);
}

TEST(LocalDirstatePersistence, loadFromFileWithWellFormattedData) {
  TemporaryFile storageFile;

  dirstate::DirstateData dirstateData;
  dirstateData.directives = {
      {"add.txt", dirstate::HgUserStatusDirectiveValue::Add},
      {"remove.txt", dirstate::HgUserStatusDirectiveValue::Remove}};
  auto serializedData = CompactSerializer::serialize<std::string>(dirstateData);
  folly::writeFull(
      storageFile.fd(), serializedData.data(), serializedData.size());

  AbsolutePath storageFilePath(storageFile.path().c_str());
  LocalDirstatePersistence persistence(storageFilePath);
  auto directives = persistence.load();
  std::unordered_map<RelativePath, HgUserStatusDirective> expectedDirectives = {
      {RelativePath("add.txt"), HgUserStatusDirective::ADD},
      {RelativePath("remove.txt"), HgUserStatusDirective::REMOVE},
  };
  EXPECT_EQ(expectedDirectives, directives);
}

TEST(LocalDirstatePersistence, attemptLoadFromNonExistentFile) {
  AbsolutePath storageFilePath;
  {
    TemporaryFile storageFile;
    storageFilePath = AbsolutePath(storageFile.path().c_str());
  }
  LocalDirstatePersistence persistence(storageFilePath);
  auto directives = persistence.load();
  EXPECT_EQ(0, directives.size());
}
