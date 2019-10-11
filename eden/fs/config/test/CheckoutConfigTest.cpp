/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/CheckoutConfig.h"

#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/utils/PathFuncs.h"

using facebook::eden::AbsolutePath;
using facebook::eden::BindMount;
using facebook::eden::CheckoutConfig;
using facebook::eden::Hash;
using facebook::eden::RelativePath;
using folly::StringPiece;

namespace {

using folly::test::TemporaryDirectory;

class CheckoutConfigTest : public ::testing::Test {
 protected:
  std::unique_ptr<TemporaryDirectory> edenDir_;
  folly::fs::path clientDir_;
  folly::fs::path mountPoint_;
  folly::fs::path configDotToml_;

  void SetUp() override {
    edenDir_ = std::make_unique<TemporaryDirectory>("eden_config_test_");
    clientDir_ = edenDir_->path() / "client";
    folly::fs::create_directory(clientDir_);
    mountPoint_ = "/tmp/someplace";

    auto snapshotPath = clientDir_ / "SNAPSHOT";
    auto snapshotContents = folly::StringPiece{
        "eden\00\00\00\01"
        "\x12\x34\x56\x78\x12\x34\x56\x78\x12\x34"
        "\x56\x78\x12\x34\x56\x78\x12\x34\x56\x78",
        28};
    folly::writeFile(snapshotContents, snapshotPath.c_str());

    configDotToml_ = clientDir_ / "config.toml";
    auto localData =
        "[repository]\n"
        "path = \"/data/users/carenthomas/fbsource\"\n"
        "type = \"git\"\n"
        "[bind-mounts]\n"
        "my-path = \"path/to-my-path\"\n";
    folly::writeFile(folly::StringPiece{localData}, configDotToml_.c_str());
  }

  void TearDown() override {
    edenDir_.reset();
  }

  template <typename ExceptionType = std::runtime_error>
  void testBadSnapshot(StringPiece contents, const char* errorRegex);
};
} // namespace

TEST_F(CheckoutConfigTest, testLoadFromClientDirectory) {
  auto config = CheckoutConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()}, AbsolutePath{clientDir_.string()});

  auto parents = config->getParentCommits();
  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"}, parents.parent1());
  EXPECT_EQ(std::nullopt, parents.parent2());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  auto pathInClientDir = clientDir_ / "bind-mounts" / "my-path";

  expectedBindMounts.emplace_back(
      BindMount{AbsolutePath{pathInClientDir.c_str()},
                AbsolutePath{"/tmp/someplace/path/to-my-path"}});
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}

TEST_F(CheckoutConfigTest, testLoadFromClientDirectoryWithNoBindMounts) {
  // Overwrite config.toml with no bind-mounts entry.
  auto data =
      "[repository]\n"
      "path = \"/data/users/carenthomas/fbsource\"\n"
      "type = \"git\"\n";
  folly::writeFile(folly::StringPiece{data}, configDotToml_.c_str());

  auto config = CheckoutConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()}, AbsolutePath{clientDir_.string()});

  auto parents = config->getParentCommits();
  EXPECT_EQ(
      Hash{"1234567812345678123456781234567812345678"}, parents.parent1());
  EXPECT_EQ(std::nullopt, parents.parent2());
  EXPECT_EQ("/tmp/someplace", config->getMountPath());

  std::vector<BindMount> expectedBindMounts;
  EXPECT_EQ(expectedBindMounts, config->getBindMounts());
}

TEST_F(CheckoutConfigTest, testMultipleParents) {
  auto config = CheckoutConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()}, AbsolutePath{clientDir_.string()});

  // Overwrite the SNAPSHOT file to indicate that there are two parents
  auto snapshotContents = folly::StringPiece{
      "eden\00\00\00\01"
      "\x99\x88\x77\x66\x55\x44\x33\x22\x11\x00"
      "\xaa\xbb\xcc\xdd\xee\xff\xab\xcd\xef\x99"
      "\xab\xcd\xef\x98\x76\x54\x32\x10\x01\x23"
      "\x45\x67\x89\xab\xcd\xef\x00\x11\x22\x33",
      48};
  auto snapshotPath = clientDir_ / "SNAPSHOT";
  folly::writeFile(snapshotContents, snapshotPath.c_str());

  auto parents = config->getParentCommits();
  EXPECT_EQ(
      Hash{"99887766554433221100aabbccddeeffabcdef99"}, parents.parent1());
  EXPECT_EQ(
      Hash{"abcdef98765432100123456789abcdef00112233"}, parents.parent2());
}

TEST_F(CheckoutConfigTest, testWriteSnapshot) {
  auto config = CheckoutConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()}, AbsolutePath{clientDir_.string()});

  Hash hash1{"99887766554433221100aabbccddeeffabcdef99"};
  Hash hash2{"abcdef98765432100123456789abcdef00112233"};
  Hash zeroHash{};

  // Write out a single parent and read it back
  config->setParentCommits(hash1);
  auto parents = config->getParentCommits();
  EXPECT_EQ(hash1, parents.parent1());
  EXPECT_EQ(std::nullopt, parents.parent2());

  // Change the parent
  config->setParentCommits(hash2);
  parents = config->getParentCommits();
  EXPECT_EQ(hash2, parents.parent1());
  EXPECT_EQ(std::nullopt, parents.parent2());

  // Set multiple parents
  config->setParentCommits(hash1, hash2);
  parents = config->getParentCommits();
  EXPECT_EQ(hash1, parents.parent1());
  EXPECT_EQ(hash2, parents.parent2());

  // We should be able to distinguish between the second parent being the
  // 0-hash and between not being set at all.
  config->setParentCommits(hash2, zeroHash);
  parents = config->getParentCommits();
  EXPECT_EQ(hash2, parents.parent1());
  EXPECT_EQ(zeroHash, parents.parent2());

  // Move back to a single parent
  config->setParentCommits(hash1);
  parents = config->getParentCommits();
  EXPECT_EQ(hash1, parents.parent1());
  EXPECT_EQ(std::nullopt, parents.parent2());
}

template <typename ExceptionType>
void CheckoutConfigTest::testBadSnapshot(
    StringPiece contents,
    const char* errorRegex) {
  SCOPED_TRACE(
      folly::to<std::string>("SNAPSHOT contents: ", folly::hexlify(contents)));
  folly::writeFile(contents, (clientDir_ / "SNAPSHOT").c_str());

  auto config = CheckoutConfig::loadFromClientDirectory(
      AbsolutePath{mountPoint_.string()}, AbsolutePath{clientDir_.string()});
  EXPECT_THROW_RE(config->getParentCommits(), ExceptionType, errorRegex);
}

TEST_F(CheckoutConfigTest, testBadSnapshot) {
  testBadSnapshot("eden", "SNAPSHOT file is too short");
  testBadSnapshot(StringPiece{"eden\0\0\0", 7}, "SNAPSHOT file is too short");
  testBadSnapshot(
      StringPiece{"eden\0\0\0\1", 8},
      "unexpected length for eden SNAPSHOT file");
  testBadSnapshot(
      StringPiece{"eden\0\0\0\x0exyza", 12},
      "unsupported eden SNAPSHOT file format \\(version 14\\)");
  testBadSnapshot(
      StringPiece{"eden\00\00\00\01"
                  "\x99\x88\x77\x66\x55\x44\x33\x22\x11\x00"
                  "\xaa\xbb\xcc\xdd\xee\xff\xab\xcd\xef\x99"
                  "\xab\xcd\xef\x98\x76\x54\x32\x10\x01\x23"
                  "\x45\x67\x89\xab\xcd\xef\x00\x11\x22",
                  47},
      "unexpected length for eden SNAPSHOT file");
  testBadSnapshot(
      StringPiece{"eden\00\00\00\01"
                  "\x99\x88\x77\x66\x55\x44\x33\x22\x11\x00"
                  "\xaa\xbb\xcc\xdd\xee\xff\xab\xcd\xef\x99"
                  "\xab\xcd\xef\x98\x76\x54\x32\x10\x01\x23"
                  "\x45\x67\x89\xab\xcd\xef\x00\x11\x22\x33\x44",
                  49},
      "unexpected length for eden SNAPSHOT file");

  // The error type and message for this will probably change in the future
  // when we drop support for the legacy SNAPSHOT file format (of a 40-byte
  // ASCII string containing the snapshot hash).
  testBadSnapshot<std::invalid_argument>("ede", "incorrect data size for Hash");
  testBadSnapshot<std::invalid_argument>(
      StringPiece{"xden\00\00\00\01"
                  "\x99\x88\x77\x66\x55\x44\x33\x22\x11\x00"
                  "\xaa\xbb\xcc\xdd\xee\xff\xab\xcd\xef\x99"
                  "\xab\xcd\xef\x98\x76\x54\x32\x10\x01\x23"
                  "\x45\x67\x89\xab\xcd\xef\x00\x11\x22\x33",
                  48},
      "incorrect data size for Hash");
}
