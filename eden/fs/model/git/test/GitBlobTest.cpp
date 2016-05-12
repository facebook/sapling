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
#include <gtest/gtest.h>
#include "crypto/lib/cpp/CryptoHelper.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/git/GitBlob.h"

using facebook::eden::Hash;
using folly::StringPiece;
using std::string;

TEST(GitBlob, testDeserialize) {
  string blobHash("3a8f8eb91101860fd8484154885838bf322964d0");
  Hash hash(blobHash);

  string contents("{\n  \"breakConfig\": true\n}\n");
  auto gitBlobObjectStr = folly::to<string>(string("blob 26\x00", 8), contents);
  folly::ByteRange gitBlobObject = folly::StringPiece{gitBlobObjectStr};

  auto blob = deserializeGitBlob(hash, gitBlobObject);
  EXPECT_EQ(hash, blob->getHash());
  EXPECT_EQ(contents, StringPiece{blob->getContents().clone()->coalesce()});
  EXPECT_EQ(
      blobHash, CryptoHelper::bin2hex(CryptoHelper::sha1(gitBlobObjectStr)))
      << "SHA-1 of contents should match key";
}
