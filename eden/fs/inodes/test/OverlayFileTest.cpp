/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/test/OverlayTestUtil.h"

#include <folly/Expected.h>
#include <folly/Range.h>
#include <folly/testing/TestUtil.h>
#include <gtest/gtest.h>
#include <array>
#include <string_view>

#include "eden/common/testharness/TempFile.h"
#include "eden/fs/telemetry/EdenFsEventsLogger.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::literals::string_piece_literals::operator""_sp;

class OverlayFileTest : public ::testing::Test {
 public:
  OverlayFileTest() : testDir_{makeTempDir("eden_overlay_file_test_")} {
    overlay_ = Overlay::create(
        canonicalPath(testDir_.path().string()),
        kPathMapDefaultCaseSensitive,
        InodeCatalogType::Legacy,
        makeTestEdenFsEventsLogger(),
        /*errorLogger=*/noopErrorLogger_,
        makeRefPtr<EdenStats>(),
        *EdenConfig::createTestEdenConfig());
    overlay_
        ->initialize(
            std::make_shared<ReloadableConfig>(
                EdenConfig::createTestEdenConfig()))
        .get();
  }

  OverlayFile createFile() {
    auto ino = overlay_->allocateInodeNumber();
    return overlay_->createOverlayFile(ino, folly::ByteRange{"contents"_sp});
  }

  std::string readContents(const OverlayFile& file) {
    EXPECT_TRUE(
        file.lseek(FsFileContentStore::kHeaderLength, SEEK_SET).hasValue());
    auto contents = file.readFile();
    EXPECT_TRUE(contents.hasValue());
    return contents.value();
  }

  folly::test::TemporaryDirectory testDir_;
  ErrorLogger noopErrorLogger_ = makeTestErrorLogger();
  std::shared_ptr<Overlay> overlay_;
};

TEST_F(OverlayFileTest, fstat) {
  auto file = createFile();

  auto st = file.fstat();
  ASSERT_TRUE(st.hasValue());
  EXPECT_EQ(
      st.value().st_size,
      8 + static_cast<FileOffset>(FsFileContentStore::kHeaderLength));
  EXPECT_EQ(readContents(file), "contents");
}

TEST_F(OverlayFileTest, preadNoInt) {
  auto file = createFile();
  std::array<char, 16> buf{};

  auto ret = file.preadNoInt(buf.data(), 1, FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 1);
  EXPECT_EQ(std::string_view(buf.data(), 1), "c");

  // A read past the end returns the bytes that exist.
  ret = file.preadNoInt(
      buf.data(), buf.size(), 2 + FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 6);
  EXPECT_EQ(std::string_view(buf.data(), 6), "ntents");

  // A read that starts past the end returns nothing.
  ret = file.preadNoInt(
      buf.data(), buf.size(), 20 + FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 0);
}

TEST_F(OverlayFileTest, lseek) {
  auto file = createFile();

  auto ret = file.lseek(FsFileContentStore::kHeaderLength, SEEK_SET);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), FsFileContentStore::kHeaderLength);
  EXPECT_EQ(file.readFile().value(), "contents");
}

TEST_F(OverlayFileTest, pwritev) {
  auto file = createFile();

  char data[] = "new contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);
  auto ret = file.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 13);
  EXPECT_EQ(readContents(file), "new contents" + std::string(1, '\0'));
}

TEST_F(OverlayFileTest, ftruncate) {
  auto file = createFile();

  auto ret = file.ftruncate(3 + FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 0);
  EXPECT_EQ(readContents(file), "con");
}

TEST_F(OverlayFileTest, fsync) {
  auto file = createFile();

  EXPECT_TRUE(file.fsync().hasValue());
  EXPECT_TRUE(file.fdatasync().hasValue());
  EXPECT_EQ(readContents(file), "contents");
}

#ifdef __linux__
// fallocate is only registered for FUSE (eden/fs/fuse/FuseChannel.cpp) and
// OverlayFile::fallocate() only supports Linux.
TEST_F(OverlayFileTest, fallocate) {
  auto file = createFile();

  auto ret = file.fallocate(0, 10 + FsFileContentStore::kHeaderLength);
  ASSERT_TRUE(ret.hasValue());
  EXPECT_EQ(ret.value(), 0);
  EXPECT_EQ(readContents(file), "contents" + std::string(2, '\0'));
}
#endif

#endif
