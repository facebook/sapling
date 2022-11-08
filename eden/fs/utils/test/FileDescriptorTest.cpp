/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileDescriptor.h"
#include <folly/String.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
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

  AbsolutePath fileName =
      canonicalPath((dir.path() / "file.txt").generic_string());

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::writeFile());
    testWritev(f);
  }

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::readFile());
    testReadv(f);
  }
}

TEST(FileDescriptor, readFullFile) {
  std::vector<uint8_t> expect;

  expect.reserve(2 * 1024 * 1024);
  for (size_t i = 0; i < expect.capacity(); ++i) {
    expect.push_back(uint8_t(i & 0xff));
  }

  auto dir = makeTempDir();
  AbsolutePath fileName =
      canonicalPath((dir.path() / "file.txt").generic_string());

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::writeFile());
    f.writeFull(expect.data(), expect.size()).throwUnlessValue();
  }

  {
    auto f = FileDescriptor::open(fileName, OpenFileHandleOptions::readFile());
    std::vector<uint8_t> got;
    got.resize(expect.size());

    f.readFull(got.data(), got.size()).throwUnlessValue();

    EXPECT_EQ(got, expect);
  }
}

TEST(FileDescriptor, readFullPipe) {
  std::vector<uint8_t> expect;

  expect.reserve(2 * 1024 * 1024);
  for (size_t i = 0; i < expect.capacity(); ++i) {
    expect.push_back(uint8_t(i & 0xff));
  }
  EXPECT_GT(expect.size(), 0);

  Pipe p;

  // The writer thread trickles the data into the pipe in discrete
  // chunks.  This increases the chances that the readFull call will
  // observe a partial read which is the trigger for a specific bug
  // we encountered.
  std::thread writer([f = std::move(p.write), &expect]() {
    constexpr size_t kChunkSize = 4096;
    for (size_t i = 0; i < expect.size(); i += kChunkSize) {
      f.writeFull(expect.data() + i, kChunkSize).throwUnlessValue();
    }
  });

  std::vector<uint8_t> got;
  got.resize(expect.size());
  p.read.readFull(got.data(), got.size()).throwUnlessValue();

  EXPECT_EQ(got, expect);

  writer.join();
}
