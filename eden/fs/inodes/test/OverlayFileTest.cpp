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

#include <fmt/format.h>
#include <folly/Exception.h>
#include <folly/Expected.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/GTest.h>
#include <algorithm>
#include <chrono>

#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/NullStructuredLogger.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::literals::string_piece_literals::operator""_sp;

class OverlayFileTest : public ::testing::Test {
 public:
  OverlayFileTest() : testDir_{makeTempDir("eden_overlay_file_test_")} {
    auto fsDir = getLocalDir() + PathComponent("fs");
    auto lmdbDir = getLocalDir() + PathComponent("lmdb");
    ::mkdir(fsDir.c_str(), 0755);
    ::mkdir(lmdbDir.c_str(), 0755);
    loadOverlay();
  }

  void loadOverlay() {
    fsOverlay = Overlay::create(
        getLocalDir() + PathComponent("fs"),
        kPathMapDefaultCaseSensitive,
        facebook::eden::InodeCatalogType::Legacy,
        INODE_CATALOG_DEFAULT,
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    fsOverlay->initialize(EdenConfig::createTestEdenConfig()).get();

    lmdbOverlay = Overlay::create(
        getLocalDir() + PathComponent("lmdb"),
        kPathMapDefaultCaseSensitive,
        facebook::eden::InodeCatalogType::LMDB,
        INODE_CATALOG_DEFAULT,
        std::make_shared<NullStructuredLogger>(),
        makeRefPtr<EdenStats>(),
        true,
        *EdenConfig::createTestEdenConfig());
    lmdbOverlay->initialize(EdenConfig::createTestEdenConfig()).get();
  }

  AbsolutePath getLocalDir() {
    return canonicalPath(testDir_.path().string());
  }

  std::string getOverlayFileContent(const OverlayFile& file) {
    auto fileContent = file.readFile();
    EXPECT_TRUE(fileContent.hasValue());
    return fileContent.value();
  }

  void checkFilesEqual(
      const OverlayFile& fsFile,
      const OverlayFile& lmdbFile,
      std::string expected) {
    fsFile.lseek(FsFileContentStore::kHeaderLength, SEEK_SET);

    auto fsContent = getOverlayFileContent(fsFile);
    auto lmdbContent = getOverlayFileContent(lmdbFile);

    EXPECT_EQ(fsContent, expected);
    EXPECT_EQ(fsContent, lmdbContent);
  }

  std::pair<OverlayFile, OverlayFile> generateOverlayFiles() {
    auto fsIno = fsOverlay->allocateInodeNumber();
    auto lmdbIno = lmdbOverlay->allocateInodeNumber();

    return std::make_pair(
        fsOverlay->createOverlayFile(fsIno, folly::ByteRange{"contents"_sp}),
        lmdbOverlay->createOverlayFile(
            lmdbIno, folly::ByteRange{"contents"_sp}));
  }

  folly::test::TemporaryDirectory testDir_;
  std::shared_ptr<Overlay> fsOverlay;
  std::shared_ptr<Overlay> lmdbOverlay;
};

TEST_F(OverlayFileTest, fstat) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fstatFs = fsFile.fstat();
  auto fstatLMDB = lmdbFile.fstat();

  EXPECT_TRUE(fstatFs.hasValue());
  EXPECT_TRUE(fstatLMDB.hasValue());

  EXPECT_EQ(
      fstatFs.value().st_size,
      8 + static_cast<FileOffset>(FsFileContentStore::kHeaderLength));
  EXPECT_EQ(fstatFs.value().st_size, fstatLMDB.value().st_size);

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, preadNoIntSmaller) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto size = 2;
  auto fsBuf = folly::IOBuf::createCombined(size);
  auto lmdbBuf = folly::IOBuf::createCombined(size);

  auto preadNoIntFs = fsFile.preadNoInt(
      fsBuf->writableBuffer(), 1, FsFileContentStore::kHeaderLength);
  auto preadNoIntLMDB = lmdbFile.preadNoInt(
      lmdbBuf->writableBuffer(), 1, FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(preadNoIntFs.hasValue());
  EXPECT_TRUE(preadNoIntLMDB.hasValue());

  EXPECT_EQ(preadNoIntFs.value(), 1);
  EXPECT_EQ(preadNoIntFs.value(), preadNoIntLMDB.value());

  fsBuf->append(preadNoIntFs.value());
  lmdbBuf->append(preadNoIntLMDB.value());

  auto expected =
      folly::IOBuf(folly::IOBuf::WRAP_BUFFER, folly::ByteRange{"c"_sp});

  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf), expected));
  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf), *(lmdbBuf)));

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, preadNoIntFull) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto size = 7;
  auto fsBuf = folly::IOBuf::createCombined(size);
  auto lmdbBuf = folly::IOBuf::createCombined(size);

  auto preadNoIntFs = fsFile.preadNoInt(
      fsBuf->writableBuffer(), size, 1 + FsFileContentStore::kHeaderLength);
  auto preadNoIntLMDB = lmdbFile.preadNoInt(
      lmdbBuf->writableBuffer(), size, 1 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(preadNoIntFs.hasValue());
  EXPECT_TRUE(preadNoIntLMDB.hasValue());

  EXPECT_EQ(preadNoIntFs.value(), 7);
  EXPECT_EQ(preadNoIntFs.value(), preadNoIntLMDB.value());

  fsBuf->append(preadNoIntFs.value());
  lmdbBuf->append(preadNoIntLMDB.value());

  auto expected =
      folly::IOBuf(folly::IOBuf::WRAP_BUFFER, folly::ByteRange{"ontents"_sp});

  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf.get()), expected));
  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf.get()), *(lmdbBuf.get())));

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, preadNoIntLonger) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto size = 11;
  auto fsBuf = folly::IOBuf::createCombined(size);
  auto lmdbBuf = folly::IOBuf::createCombined(size);

  auto preadNoIntFs = fsFile.preadNoInt(
      fsBuf->writableBuffer(), size, 2 + FsFileContentStore::kHeaderLength);
  auto preadNoIntLMDB = lmdbFile.preadNoInt(
      lmdbBuf->writableBuffer(), size, 2 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(preadNoIntFs.hasValue());
  EXPECT_TRUE(preadNoIntLMDB.hasValue());

  EXPECT_EQ(preadNoIntFs.value(), 6);
  EXPECT_EQ(preadNoIntFs.value(), preadNoIntLMDB.value());

  fsBuf->append(preadNoIntFs.value());
  lmdbBuf->append(preadNoIntLMDB.value());

  auto expected =
      folly::IOBuf(folly::IOBuf::WRAP_BUFFER, folly::ByteRange{"ntents"_sp});

  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf.get()), expected));
  EXPECT_TRUE(folly::IOBufEqualTo()(*(fsBuf.get()), *(lmdbBuf.get())));

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, lseek) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto lseekFs = fsFile.lseek(FsFileContentStore::kHeaderLength, SEEK_SET);

  // This is unimplemented in LMDBFileContentStore
  EXPECT_THROW(
      lmdbFile.lseek(FsFileContentStore::kHeaderLength, SEEK_SET), EdenError);

  EXPECT_TRUE(lseekFs.hasValue());
  EXPECT_EQ(lseekFs.value(), FsFileContentStore::kHeaderLength);
  auto fsContent = getOverlayFileContent(fsFile);
  EXPECT_EQ(fsContent, "contents");
}

TEST_F(OverlayFileTest, pwritevShorter) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  char data[] = "new";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);
  auto pwritevFs = fsFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);
  auto pwritevLMDB =
      lmdbFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(pwritevFs.hasValue());
  EXPECT_TRUE(pwritevLMDB.hasValue());

  EXPECT_EQ(pwritevFs.value(), 4);
  EXPECT_EQ(pwritevFs.value(), pwritevLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "new" + std::string(1, '\0') + "ents");
}

TEST_F(OverlayFileTest, pwritevFull) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  char data[] = "contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);
  auto pwritevFs = fsFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);
  auto pwritevLMDB =
      lmdbFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(pwritevFs.hasValue());
  EXPECT_TRUE(pwritevLMDB.hasValue());

  EXPECT_EQ(pwritevFs.value(), 9);
  EXPECT_EQ(pwritevFs.value(), pwritevLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "contents" + std::string(1, '\0'));
}

TEST_F(OverlayFileTest, pwritevLonger) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  char data[] = "new contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);
  auto pwritevFs = fsFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);
  auto pwritevLMDB =
      lmdbFile.pwritev(&iov, 1, FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(pwritevFs.hasValue());
  EXPECT_TRUE(pwritevLMDB.hasValue());

  EXPECT_EQ(pwritevFs.value(), 13);
  EXPECT_EQ(pwritevFs.value(), pwritevLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "new contents" + std::string(1, '\0'));
}

TEST_F(OverlayFileTest, ftruncateShorter) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto ftruncateFs = fsFile.ftruncate(3 + FsFileContentStore::kHeaderLength);
  auto ftruncateLMDB =
      lmdbFile.ftruncate(3 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(ftruncateFs.hasValue());
  EXPECT_TRUE(ftruncateLMDB.hasValue());

  EXPECT_EQ(ftruncateFs.value(), 0);
  EXPECT_EQ(ftruncateFs.value(), ftruncateLMDB.value());

  checkFilesEqual(fsFile, lmdbFile, "con");
}

TEST_F(OverlayFileTest, ftruncateFull) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto ftruncateFs = fsFile.ftruncate(8 + FsFileContentStore::kHeaderLength);
  auto ftruncateLMDB =
      lmdbFile.ftruncate(8 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(ftruncateFs.hasValue());
  EXPECT_TRUE(ftruncateLMDB.hasValue());

  EXPECT_EQ(ftruncateFs.value(), 0);
  EXPECT_EQ(ftruncateFs.value(), ftruncateLMDB.value());

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, ftruncateLonger) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto ftruncateFs = fsFile.ftruncate(10 + FsFileContentStore::kHeaderLength);
  auto ftruncateLMDB =
      lmdbFile.ftruncate(10 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(ftruncateFs.hasValue());
  EXPECT_TRUE(ftruncateLMDB.hasValue());

  EXPECT_EQ(ftruncateFs.value(), 0);
  EXPECT_EQ(ftruncateFs.value(), ftruncateLMDB.value());

  checkFilesEqual(fsFile, lmdbFile, "contents" + std::string(2, '\0'));
}

TEST_F(OverlayFileTest, fsync) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fsyncFs = fsFile.fsync();
  auto fsyncLMDB = lmdbFile.fsync();

  EXPECT_TRUE(fsyncFs.hasValue());
  EXPECT_TRUE(fsyncLMDB.hasValue());

  EXPECT_EQ(fsyncFs.value(), 0);
  EXPECT_EQ(fsyncFs.value(), fsyncLMDB.value());

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

#ifdef __linux__
// Only run the fallocate tests on Linux because they are not supported on
// other platforms as per OverlayFile::fallocate(), but also because it is
// only registered in eden/fs/fuse/FuseChannel.cpp and not in
// eden/fs/nfs/Nfsd3.cpp
TEST_F(OverlayFileTest, fallocateShorter) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fallocateFs = fsFile.fallocate(0, 3 + FsFileContentStore::kHeaderLength);
  auto fallocateLMDB =
      lmdbFile.fallocate(0, 3 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(fallocateFs.hasValue());
  EXPECT_TRUE(fallocateLMDB.hasValue());

  EXPECT_EQ(fallocateFs.value(), 0);
  EXPECT_EQ(fallocateFs.value(), fallocateLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, fallocateFull) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fallocateFs = fsFile.fallocate(0, 8 + FsFileContentStore::kHeaderLength);
  auto fallocateLMDB =
      lmdbFile.fallocate(0, 8 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(fallocateFs.hasValue());
  EXPECT_TRUE(fallocateLMDB.hasValue());

  EXPECT_EQ(fallocateFs.value(), 0);
  EXPECT_EQ(fallocateFs.value(), fallocateLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, fallocateLonger) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fallocateFs =
      fsFile.fallocate(0, 10 + FsFileContentStore::kHeaderLength);
  auto fallocateLMDB =
      lmdbFile.fallocate(0, 10 + FsFileContentStore::kHeaderLength);

  EXPECT_TRUE(fallocateFs.hasValue());
  EXPECT_TRUE(fallocateLMDB.hasValue());

  EXPECT_EQ(fallocateFs.value(), 0);
  EXPECT_EQ(fallocateFs.value(), fallocateLMDB.value());
  checkFilesEqual(fsFile, lmdbFile, "contents" + std::string(2, '\0'));
}
#endif

TEST_F(OverlayFileTest, fdatasync) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  auto fdatasyncFs = fsFile.fdatasync();
  auto fdatasyncLMDB = lmdbFile.fdatasync();

  EXPECT_TRUE(fdatasyncFs.hasValue());
  EXPECT_TRUE(fdatasyncLMDB.hasValue());

  EXPECT_EQ(fdatasyncFs.value(), 0);
  EXPECT_EQ(fdatasyncFs.value(), fdatasyncLMDB.value());

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

TEST_F(OverlayFileTest, readFile) {
  const auto& [fsFile, lmdbFile] = generateOverlayFiles();

  checkFilesEqual(fsFile, lmdbFile, "contents");
}

#endif
