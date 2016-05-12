/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/String.h>
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include "crypto/lib/cpp/CryptoHelper.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/LocalStore.h"

using namespace facebook::eden;
using TempDir = folly::test::TemporaryDirectory;

using facebook::eden::Hash;
using std::string;

string toBinaryHash(string hex);

TEST(LocalStore, testReadAndWriteBlob) {
  TempDir tmp;
  LocalStore store(tmp.path().string());

  string blobHash("3a8f8eb91101860fd8484154885838bf322964d0");
  Hash hash(blobHash);

  string contents("{\n  \"breakConfig\": true\n}\n");
  auto gitBlobObjectStr = folly::to<string>(string("blob 26\x00", 8), contents);

  Hash sha1(CryptoHelper::bin2hex(CryptoHelper::sha1(gitBlobObjectStr)));
  store.putBlob(hash, folly::StringPiece{gitBlobObjectStr}, sha1);

  auto blob = store.getBlob(hash);
  EXPECT_EQ(hash, blob->getHash());
  EXPECT_EQ(
      contents, blob->getContents().clone()->moveToFbString().toStdString());

  EXPECT_EQ(sha1, *store.getSha1ForBlob(hash).get());
}

TEST(LocalStore, testReadsAndWriteTree) {
  TempDir tmp;
  LocalStore store(tmp.path().string());

  Hash hash(folly::StringPiece{"8e073e366ed82de6465d1209d3f07da7eebabb93"});

  auto gitTreeObject = folly::to<string>(
      string("tree 424\x00", 9),

      string("100644 .babelrc\x00", 16),
      toBinaryHash("3a8f8eb91101860fd8484154885838bf322964d0"),

      string("100644 .flowconfig\x00", 19),
      toBinaryHash("3610882f48696cc7ca0835929511c9db70acbec6"),

      string("100644 README.md\x00", 17),
      toBinaryHash("c5f15617ed29cd35964dc197a7960aeaedf2c2d5"),

      string("40000 lib\x00", 10),
      toBinaryHash("e95798e17f694c227b7a8441cc5c7dae50a187d0"),

      string("100755 nuclide-start-server\x00", 28),
      toBinaryHash("006babcf5734d028098961c6f4b6b6719656924b"),

      string("100644 package.json\x00", 20),
      toBinaryHash("582591e0f0d92cb63a85156e39abd43ebf103edc"),

      string("40000 scripts\x00", 14),
      toBinaryHash("e664fd28e60a0da25739fdf732f412ab3e91d1e1"),

      string("100644 services-3.json\x00", 23),
      toBinaryHash("3ead3c6cd723f4867bef4444ba18e6ffbf0f711a"),

      string("100644 services-config.json\x00", 28),
      toBinaryHash("bbc8e67499b7f3e1ea850eeda1253be7da5c9199"),

      string("40000 spec\x00", 11),
      toBinaryHash("3bae53a99d080dd851f78e36eb343320091a3d57"),

      string("100644 xdebug.ini\x00", 18),
      toBinaryHash("9ed5bbccd1b9b0077561d14c0130dc086ab27e04"));

  store.putTree(hash, folly::StringPiece{gitTreeObject});
  auto tree = store.getTree(hash);
  EXPECT_EQ(Hash("8e073e366ed82de6465d1209d3f07da7eebabb93"), tree->getHash());
  EXPECT_EQ(11, tree->getTreeEntries().size());

  auto readmeEntry = tree->getEntryAt(2);
  EXPECT_EQ(
      Hash("c5f15617ed29cd35964dc197a7960aeaedf2c2d5"), readmeEntry.getHash());
  EXPECT_EQ("README.md", readmeEntry.getName());
  EXPECT_EQ(TreeEntryType::BLOB, readmeEntry.getType());
  EXPECT_EQ(FileType::REGULAR_FILE, readmeEntry.getFileType());
  EXPECT_EQ(0b0110, readmeEntry.getOwnerPermissions());
}

string toBinaryHash(string hex) {
  string bytes;
  folly::unhexlify(hex, bytes);
  return bytes;
}
