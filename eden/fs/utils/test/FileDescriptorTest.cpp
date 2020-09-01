/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileDescriptor.h"
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <list>
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/Pipe.h"

using namespace facebook::eden;
namespace {
folly::StringPiece hello("hello");
folly::StringPiece there("there");
} // namespace

void testReadWrite(FileDescriptor& read, FileDescriptor& write) {
  EXPECT_EQ(hello.size(), write.write(hello.data(), hello.size()).value());

  char buf[128];
  EXPECT_EQ(hello.size(), read.read(buf, sizeof(buf)).value());
}

TEST(FileDescriptor, pipeReadWrite) {
  Pipe p;
  testReadWrite(p.read, p.write);
}

TEST(FileDescriptor, socketPairReadWrite) {
  SocketPair p;
  testReadWrite(p.read, p.write);
}

void testWritev(FileDescriptor& write) {
  iovec iov[2];
  iov[0].iov_base = const_cast<char*>(hello.data());
  iov[0].iov_len = hello.size();

  iov[1].iov_base = const_cast<char*>(there.data());
  iov[1].iov_len = there.size();

  EXPECT_EQ(
      hello.size() + there.size(),
      write.writevFull(iov, std::size(iov)).value());
}

void testReadv(FileDescriptor& read) {
  iovec iov[2];
  char buf1[2];
  char buf2[30];

  iov[0].iov_base = buf1;
  iov[0].iov_len = sizeof(buf1);

  iov[1].iov_base = buf2;
  iov[1].iov_len = sizeof(buf2);

  EXPECT_EQ(
      hello.size() + there.size(), read.readv(iov, std::size(iov)).value());

  EXPECT_EQ("he", folly::StringPiece(buf1, 2));
  EXPECT_EQ("llothere", folly::StringPiece(buf2, 8));
}

void testReadvWritev(FileDescriptor& read, FileDescriptor& write) {
  testWritev(write);
  testReadv(read);
}

TEST(FileDescriptor, pipeReadvWritev) {
  Pipe p;
  testReadWrite(p.read, p.write);
}

TEST(FileDescriptor, socketPairReadvWritev) {
  SocketPair p;
  testReadWrite(p.read, p.write);
}

TEST(FileDescriptor, fileReadvWritev) {
  auto dir = makeTempDir();

  AbsolutePath fileName((dir.path() / "file.txt").generic_string());

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::writeFile());
    testWritev(f);
  }

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::readFile());
    testReadv(f);
  }
}
