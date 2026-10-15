/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include <gtest/gtest.h>
#include <thrift/lib/cpp2/protocol/CompactProtocol.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include <folly/FileUtil.h>
#include <folly/io/IOBufQueue.h>
#include <folly/testing/TestUtil.h>

#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/EdenError.h"

using namespace facebook::eden;
using apache::thrift::CompactProtocolWriter;
using apache::thrift::CompactSerializer;
using apache::thrift::protocol::TType;

class FsInodeCatalogEntryTest : public ::testing::Test {
 protected:
  void SetUp() override {
    store_ = std::make_unique<FsFileContentStore>(
        canonicalPath(testDir_.path().string()));
    store_->initialize(/*createIfNonExisting=*/true);
    catalog_ = std::make_unique<FsInodeCatalog>(store_.get());
  }

  void TearDown() override {
    catalog_.reset();
    store_->close();
    store_.reset();
  }

  folly::test::TemporaryDirectory testDir_;
  std::unique_ptr<FsFileContentStore> store_;
  std::unique_ptr<FsInodeCatalog> catalog_;
};

// Collect entries from loadOverlayEntries into a map for easy comparison.
std::map<std::string, overlay::OverlayEntry> collectEntries(
    FsInodeCatalog& catalog,
    InodeNumber ino) {
  std::map<std::string, overlay::OverlayEntry> result;
  bool found = catalog.loadOverlayEntries(
      ino, [&](size_t count, InodeCatalog::OverlayEntryIterator iterate) {
        iterate(
            [&](const std::string& name, const overlay::OverlayEntry& entry) {
              result.emplace(name, entry);
            });
        EXPECT_EQ(count, result.size());
      });
  if (!found) {
    return {};
  }
  return result;
}

// Build entries source callback from an OverlayDir.
void saveFromOverlayDir(
    FsInodeCatalog& catalog,
    InodeNumber ino,
    const overlay::OverlayDir& odir) {
  catalog.saveOverlayEntries(
      ino,
      odir.entries()->size(),
      [&](InodeCatalog::OverlayEntryVisitor visitor) {
        for (const auto& [name, entry] : *odir.entries()) {
          visitor(name, entry);
        }
      });
}

void expectEntriesEqual(
    const std::map<std::string, overlay::OverlayEntry>& a,
    const std::map<std::string, overlay::OverlayEntry>& b) {
  ASSERT_EQ(a.size(), b.size());
  for (const auto& [name, entryA] : a) {
    auto it = b.find(name);
    ASSERT_NE(it, b.end()) << "Missing entry: " << name;
    const auto& entryB = it->second;
    EXPECT_EQ(*entryA.mode(), *entryB.mode());
    EXPECT_EQ(*entryA.inodeNumber(), *entryB.inodeNumber());
    EXPECT_EQ(entryA.hash().has_value(), entryB.hash().has_value());
    if (entryA.hash().has_value()) {
      EXPECT_EQ(*entryA.hash(), *entryB.hash());
    }
  }
}

TEST_F(FsInodeCatalogEntryTest, saveEntriesLoadViaOldPath) {
  // Save via saveOverlayEntries, load via loadOverlayDir.
  overlay::OverlayDir odir;
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = 2;
    odir.entries()->emplace("file.txt", std::move(e));
  }
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFDIR | 0755;
    e.inodeNumber() = 3;
    e.hash() = std::string("abcdef1234567890abcd");
    odir.entries()->emplace("subdir", std::move(e));
  }

  saveFromOverlayDir(*catalog_, InodeNumber{10}, odir);

  auto loaded = catalog_->loadOverlayDir(InodeNumber{10});
  ASSERT_TRUE(loaded.has_value());
  EXPECT_EQ(loaded->entries()->size(), 2);

  auto& entries = *loaded->entries();
  EXPECT_EQ(*entries["file.txt"].mode(), S_IFREG | 0644);
  EXPECT_EQ(*entries["file.txt"].inodeNumber(), 2);
  EXPECT_FALSE(entries["file.txt"].hash().has_value());

  EXPECT_EQ(*entries["subdir"].mode(), S_IFDIR | 0755);
  EXPECT_EQ(*entries["subdir"].inodeNumber(), 3);
  EXPECT_EQ(*entries["subdir"].hash(), "abcdef1234567890abcd");
}

TEST_F(FsInodeCatalogEntryTest, saveViaOldPathLoadEntries) {
  // Save via saveOverlayDir, load via loadOverlayEntries.
  overlay::OverlayDir odir;
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = 2;
    odir.entries()->emplace("file.txt", std::move(e));
  }
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFDIR | 0755;
    e.inodeNumber() = 3;
    e.hash() = std::string("abcdef1234567890abcd");
    odir.entries()->emplace("subdir", std::move(e));
  }

  catalog_->saveOverlayDir(InodeNumber{10}, overlay::OverlayDir{odir});

  auto entries = collectEntries(*catalog_, InodeNumber{10});
  ASSERT_EQ(entries.size(), 2);

  EXPECT_EQ(*entries["file.txt"].mode(), S_IFREG | 0644);
  EXPECT_EQ(*entries["file.txt"].inodeNumber(), 2);
  EXPECT_FALSE(entries["file.txt"].hash().has_value());

  EXPECT_EQ(*entries["subdir"].mode(), S_IFDIR | 0755);
  EXPECT_EQ(*entries["subdir"].inodeNumber(), 3);
  EXPECT_EQ(*entries["subdir"].hash(), "abcdef1234567890abcd");
}

TEST_F(FsInodeCatalogEntryTest, roundTripEntries) {
  // Save via saveOverlayEntries, load via loadOverlayEntries.
  overlay::OverlayDir odir;
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = 5;
    odir.entries()->emplace("alpha", std::move(e));
  }
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFDIR | 0755;
    e.inodeNumber() = 6;
    e.hash() = std::string("deadbeef12345678dead");
    odir.entries()->emplace("beta", std::move(e));
  }
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0755;
    e.inodeNumber() = 7;
    odir.entries()->emplace("gamma", std::move(e));
  }

  saveFromOverlayDir(*catalog_, InodeNumber{20}, odir);
  auto loaded = collectEntries(*catalog_, InodeNumber{20});
  expectEntriesEqual(*odir.entries(), loaded);
}

TEST_F(FsInodeCatalogEntryTest, loadInodeInfoAndEntries) {
  overlay::OverlayDir odir;
  overlay::OverlayEntry entry;
  entry.mode() = S_IFREG | 0644;
  entry.inodeNumber() = 5;
  odir.entries()->emplace("alpha", std::move(entry));
  saveFromOverlayDir(*catalog_, InodeNumber{20}, odir);

  std::map<std::string, overlay::OverlayEntry> entries;
  auto info = catalog_->loadInodeInfoAndEntries(
      InodeNumber{20},
      [&](size_t count, InodeCatalog::OverlayEntryIterator iterate) {
        EXPECT_EQ(1, count);
        iterate([&](const std::string& name, const auto& loadedEntry) {
          entries.emplace(name, loadedEntry);
        });
      });

  ASSERT_TRUE(info.has_value());
  EXPECT_EQ(fsck::InodeType::Dir, info->type);
  expectEntriesEqual(*odir.entries(), entries);
}

TEST_F(FsInodeCatalogEntryTest, loadFileInfoDoesNotVisitEntries) {
  constexpr folly::StringPiece contents{"contents"};
  store_->createOverlayFile(
      InodeNumber{21},
      folly::ByteRange{
          reinterpret_cast<const uint8_t*>(contents.data()), contents.size()});

  auto info = catalog_->loadInodeInfoAndEntries(
      InodeNumber{21}, [](size_t, InodeCatalog::OverlayEntryIterator) {
        FAIL() << "file inode unexpectedly produced directory entries";
      });

  ASSERT_TRUE(info.has_value());
  EXPECT_EQ(fsck::InodeType::File, info->type);
}

TEST_F(FsInodeCatalogEntryTest, emptyDirectory) {
  overlay::OverlayDir odir;

  saveFromOverlayDir(*catalog_, InodeNumber{30}, odir);

  // loadOverlayEntries
  auto loaded = collectEntries(*catalog_, InodeNumber{30});
  EXPECT_EQ(loaded.size(), 0);

  // loadOverlayDir
  auto loadedDir = catalog_->loadOverlayDir(InodeNumber{30});
  ASSERT_TRUE(loadedDir.has_value());
  EXPECT_EQ(loadedDir->entries()->size(), 0);
}

TEST_F(FsInodeCatalogEntryTest, rejectsDuplicateEntryNames) {
  constexpr InodeNumber ino{32};
  catalog_->saveOverlayDir(ino, overlay::OverlayDir{});

  folly::IOBufQueue queue;
  CompactProtocolWriter writer;
  writer.setOutput(&queue);
  writer.writeStructBegin("OverlayDir");
  writer.writeFieldBegin("entries", TType::T_MAP, 1);
  writer.writeMapBegin(TType::T_STRING, TType::T_STRUCT, 2);
  for (uint64_t inodeNumber : {2, 3}) {
    writer.writeString("file.txt");
    writer.writeStructBegin("OverlayEntry");
    writer.writeFieldBegin("mode", TType::T_I32, 1);
    writer.writeI32(S_IFREG | 0644);
    writer.writeFieldEnd();
    writer.writeFieldBegin("inodeNumber", TType::T_I64, 2);
    writer.writeI64(inodeNumber);
    writer.writeFieldEnd();
    writer.writeFieldStop();
    writer.writeStructEnd();
  }
  writer.writeMapEnd();
  writer.writeFieldEnd();
  writer.writeFieldStop();
  writer.writeStructEnd();

  auto path = store_->getAbsoluteFilePath(ino);
  std::string contents;
  ASSERT_TRUE(folly::readFile(path.c_str(), contents));
  ASSERT_GE(contents.size(), FsFileContentStore::kHeaderLength);
  contents.resize(FsFileContentStore::kHeaderLength);
  contents.append(queue.move()->to<std::string>());
  ASSERT_TRUE(folly::writeFile(contents, path.c_str()));

  try {
    catalog_->loadOverlayDir(ino);
    FAIL() << "expected duplicate-name corruption";
  } catch (const OverlayDataCorruption& ex) {
    const std::string message = ex.what();
    EXPECT_NE(std::string::npos, message.find("for inode 32"));
    EXPECT_NE(std::string::npos, message.find("hex \"66696c652e747874\""));
    EXPECT_NE(
        std::string::npos, message.find("first inode 2, duplicate inode 3"));
  }
}

TEST_F(FsInodeCatalogEntryTest, rejectsTrailingDataAfterVisitingValidEntries) {
  overlay::OverlayDir odir;
  overlay::OverlayEntry entry;
  entry.mode() = S_IFREG | 0644;
  entry.inodeNumber() = 2;
  odir.entries()->emplace("file.txt", std::move(entry));
  catalog_->saveOverlayDir(InodeNumber{32}, std::move(odir));

  auto path = store_->getAbsoluteFilePath(InodeNumber{32});
  std::string contents;
  ASSERT_TRUE(folly::readFile(path.c_str(), contents));
  contents.push_back('\0');
  ASSERT_TRUE(folly::writeFile(contents, path.c_str()));

  size_t entriesVisited = 0;
  try {
    catalog_->loadOverlayEntries(
        InodeNumber{32},
        [&](size_t, InodeCatalog::OverlayEntryIterator iterate) {
          iterate([&](const std::string&, const overlay::OverlayEntry&) {
            ++entriesVisited;
          });
        });
    FAIL() << "expected trailing-data corruption";
  } catch (const OverlayDataCorruption& ex) {
    const std::string message = ex.what();
    EXPECT_NE(
        std::string::npos, message.find("trailing overlay data for inode 32"));
    EXPECT_NE(std::string::npos, message.find("directory data offset"));
    EXPECT_NE(std::string::npos, message.find("1 bytes remain"));
  }
  EXPECT_EQ(1, entriesVisited);
}

TEST_F(
    FsInodeCatalogEntryTest,
    headerErrorsIncludeInodeAndActualAndExpectedValues) {
  // Construct independent wire bytes: identifier, big-endian version, padding.
  std::string version{"OVDR\0\0\0\7", 8};
  version.resize(64, '\0');
  std::string identifier{"BAD!\0\0\0\1", 8};
  identifier.resize(64, '\0');
  const InodeNumber ino{47};
  catalog_->saveOverlayDir(ino, overlay::OverlayDir{});
  const auto path = store_->getAbsoluteFilePath(ino);
  for (const auto& bytes : {std::string{"OV"}, identifier, version}) {
    SCOPED_TRACE(bytes.size());
    ASSERT_TRUE(folly::writeFile(bytes, path.c_str()));
    try {
      catalog_->loadOverlayDir(ino);
      FAIL() << "expected corrupt header";
    } catch (const OverlayDataCorruption& ex) {
      const std::string message = ex.what();
      EXPECT_NE(std::string::npos, message.find("inode 47"));
      if (bytes.size() == 2) {
        EXPECT_NE(std::string::npos, message.find("size=2"));
        EXPECT_NE(std::string::npos, message.find("at least 64 bytes"));
      } else if (bytes == identifier) {
        EXPECT_NE(std::string::npos, message.find("offset 0"));
        EXPECT_NE(std::string::npos, message.find("actual hex=42414421"));
        EXPECT_NE(std::string::npos, message.find("expected hex=4f564452"));
      } else {
        EXPECT_NE(std::string::npos, message.find("offset 4"));
        EXPECT_NE(std::string::npos, message.find("actual=7 expected=1"));
      }
    }
  }
}

TEST_F(FsInodeCatalogEntryTest, parseErrorsIdentifyInodeOffsetAndStage) {
  const InodeNumber ino{48};
  catalog_->saveOverlayDir(ino, overlay::OverlayDir{});
  const auto path = store_->getAbsoluteFilePath(ino);
  std::string bytes;
  ASSERT_TRUE(folly::readFile(path.c_str(), bytes));
  // 0x1b starts the entries map field but omits its map header.
  bytes.resize(FsFileContentStore::kHeaderLength);
  bytes.push_back('\x1b');
  ASSERT_TRUE(folly::writeFile(bytes, path.c_str()));

  try {
    catalog_->loadOverlayDir(ino);
    FAIL() << "expected truncated map header";
  } catch (const OverlayDataCorruption& ex) {
    const std::string message = ex.what();
    EXPECT_NE(std::string::npos, message.find("inode 48"));
    EXPECT_NE(std::string::npos, message.find("directory data offset 1"));
    EXPECT_NE(std::string::npos, message.find("while reading entries map"));
  }
}

TEST_F(FsInodeCatalogEntryTest, nonExistentReturnsFalse) {
  bool found = catalog_->loadOverlayEntries(
      InodeNumber{999}, [](uint32_t, InodeCatalog::OverlayEntryIterator) {
        FAIL() << "Should not be called for non-existent inode";
      });
  EXPECT_FALSE(found);
}

TEST_F(FsInodeCatalogEntryTest, byteCompatibility) {
  // saveOverlayEntries should produce byte-identical output to saveOverlayDir.
  // We verify this by saving via both paths and loading each with the other's
  // load method, then comparing the deserialized results.
  overlay::OverlayDir odir;
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = 2;
    odir.entries()->emplace("alpha", std::move(e));
  }
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFDIR | 0755;
    e.inodeNumber() = 3;
    e.hash() = std::string("0123456789abcdef0123");
    odir.entries()->emplace("beta", std::move(e));
  }

  // Save via old path, load via new path
  catalog_->saveOverlayDir(InodeNumber{40}, overlay::OverlayDir{odir});
  auto fromOld = collectEntries(*catalog_, InodeNumber{40});

  // Save via new path, load via old path
  saveFromOverlayDir(*catalog_, InodeNumber{41}, odir);
  auto fromNew = catalog_->loadOverlayDir(InodeNumber{41});

  ASSERT_TRUE(fromNew.has_value());
  expectEntriesEqual(*odir.entries(), fromOld);
  expectEntriesEqual(*odir.entries(), *fromNew->entries());
}

TEST_F(FsInodeCatalogEntryTest, emptyDirByteIdentical) {
  // An empty directory must serialize to exactly the same bytes whether
  // written through saveOverlayDir or saveOverlayEntries.
  overlay::OverlayDir odir;

  catalog_->saveOverlayDir(InodeNumber{60}, overlay::OverlayDir{odir});
  saveFromOverlayDir(*catalog_, InodeNumber{61}, odir);

  auto file1 = std::get<folly::File>(store_->openFileNoVerify(InodeNumber{60}));
  auto file2 = std::get<folly::File>(store_->openFileNoVerify(InodeNumber{61}));

  std::string contents1, contents2;
  ASSERT_TRUE(folly::readFile(file1.fd(), contents1));
  ASSERT_TRUE(folly::readFile(file2.fd(), contents2));
  EXPECT_EQ(contents1, contents2);
}

TEST_F(FsInodeCatalogEntryTest, singleEntryByteIdentical) {
  // A directory with one entry must serialize to exactly the same bytes
  // whether written through saveOverlayDir or saveOverlayEntries.
  overlay::OverlayDir odir;
  {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = 2;
    e.hash() = std::string("abcdef1234567890abcd");
    odir.entries()->emplace("file.txt", std::move(e));
  }

  catalog_->saveOverlayDir(InodeNumber{62}, overlay::OverlayDir{odir});
  saveFromOverlayDir(*catalog_, InodeNumber{63}, odir);

  auto file1 = std::get<folly::File>(store_->openFileNoVerify(InodeNumber{62}));
  auto file2 = std::get<folly::File>(store_->openFileNoVerify(InodeNumber{63}));

  std::string contents1, contents2;
  ASSERT_TRUE(folly::readFile(file1.fd(), contents1));
  ASSERT_TRUE(folly::readFile(file2.fd(), contents2));
  EXPECT_EQ(contents1, contents2);
}

TEST_F(FsInodeCatalogEntryTest, largeDirectory) {
  overlay::OverlayDir odir;
  for (int i = 0; i < 500; ++i) {
    overlay::OverlayEntry e;
    e.mode() = S_IFREG | 0644;
    e.inodeNumber() = i + 2;
    if (i % 3 != 0) {
      e.hash() = fmt::format("{:040x}", i);
    }
    odir.entries()->emplace(fmt::format("entry_{:04d}", i), std::move(e));
  }

  saveFromOverlayDir(*catalog_, InodeNumber{50}, odir);

  // Verify round-trip via loadOverlayEntries
  auto loaded = collectEntries(*catalog_, InodeNumber{50});
  expectEntriesEqual(*odir.entries(), loaded);

  // Verify cross-path: new save, old load
  auto loadedDir = catalog_->loadOverlayDir(InodeNumber{50});
  ASSERT_TRUE(loadedDir.has_value());
  expectEntriesEqual(*odir.entries(), *loadedDir->entries());
}

#endif
