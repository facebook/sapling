/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/model/git/GitBlob.h"
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"

using facebook::eden::Hash;
using folly::IOBuf;
using folly::StringPiece;
using std::string;

// Test deserializing from an unmanaged IOBuf, which doesn't control the
// lifetime of the underlying data
TEST(GitBlob, testDeserializeUnmanaged) {
  string blobHash("3a8f8eb91101860fd8484154885838bf322964d0");
  Hash hash(blobHash);

  string contents("{\n  \"breakConfig\": true\n}\n");
  auto gitBlobObjectStr = folly::to<string>(string("blob 26\x00", 8), contents);
  folly::ByteRange gitBlobObject = folly::StringPiece{gitBlobObjectStr};
  EXPECT_EQ(blobHash, Hash::sha1(gitBlobObject).toString())
      << "SHA-1 of contents should match key";

  IOBuf buf(IOBuf::WRAP_BUFFER, gitBlobObject);
  auto blob = deserializeGitBlob(hash, &buf);
  EXPECT_EQ(hash, blob->getHash());
  EXPECT_FALSE(blob->getContents().isShared())
      << "deserializeGitBlob() must make a copy of the buffer given "
      << "an unmanaged IOBuf as input";
  EXPECT_EQ(contents, StringPiece{blob->getContents().clone()->coalesce()});
  // Make sure the blob contents are still valid after freeing our string data.
  {
    std::string empty;
    gitBlobObjectStr.swap(empty);
    buf = IOBuf();
  }
  EXPECT_EQ(contents, StringPiece{blob->getContents().clone()->coalesce()});
}

TEST(GitBlob, testDeserializeManaged) {
  string blobHash("3a8f8eb91101860fd8484154885838bf322964d0");
  Hash hash(blobHash);

  string contents("{\n  \"breakConfig\": true\n}\n");

  auto buf = IOBuf::create(1024);
  folly::io::Appender appender(buf.get(), 0);
  appender.printf("blob %zu", contents.size());
  appender.write<uint8_t>(0);
  appender.push(StringPiece(contents));
  // Sanity check that we are the only user of the newly-created IOBuf
  EXPECT_FALSE(buf->isShared()) << "newly created IOBuf should not be shared";

  auto blob = deserializeGitBlob(hash, buf.get());
  EXPECT_EQ(hash, blob->getHash());
  EXPECT_TRUE(buf->isShared())
      << "deserializeGitBlob() should return a blob that shares the same "
      << "IOBuf data, instead of copying it";
  EXPECT_EQ(contents, StringPiece{blob->getContents().clone()->coalesce()});
  // Make sure the blob contents are still valid after freeing
  // our copy of the IOBuf
  buf.reset();
  EXPECT_EQ(contents, StringPiece{blob->getContents().clone()->coalesce()});
}
