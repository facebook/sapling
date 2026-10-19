/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <string>
#include <vector>

#include <folly/Range.h>
#include <gtest/gtest.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/fuzz/overlay/OverlayFileFuzzFixture.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

using namespace facebook::eden;
using apache::thrift::CompactSerializer;

namespace {

folly::ByteRange asRange(const std::string& bytes) {
  return folly::ByteRange{
      reinterpret_cast<const uint8_t*>(bytes.data()), bytes.size()};
}

std::string serializeEmptyDir() {
  return CompactSerializer::serialize<std::string>(overlay::OverlayDir{});
}

std::string serializeOneEntryDir() {
  overlay::OverlayDir dir;
  overlay::OverlayEntry entry;
  entry.mode() = 0100644;
  entry.inodeNumber() = 42;
  entry.isRestricted() = false;
  dir.entries()->emplace("hello", std::move(entry));
  return CompactSerializer::serialize<std::string>(dir);
}

class OverlayFuzzSeedTest : public ::testing::Test {
 protected:
  OverlayFileFuzzFixture fixture_;
  InodeNumber ino_{OverlayFileFuzzFixture::pickInode(7)};
};

// The header the fixture prepends is the header production writes.
TEST_F(OverlayFuzzSeedTest, capturedHeaderMatchesOnDiskFormat) {
  const auto& header = fixture_.validDirHeader();
  ASSERT_EQ(FsFileContentStore::kHeaderLength, header.size());
  EXPECT_EQ(
      FsFileContentStore::kHeaderIdentifierDir.str(), header.substr(0, 4));
  EXPECT_EQ(std::string("\0\0\0\1", 4), header.substr(4, 4));
  EXPECT_EQ(
      std::string(FsFileContentStore::kHeaderLength - 8, '\0'),
      header.substr(8));
}

// Seed: valid header + one-entry dir. Parsing an entry back out is only
// reachable after validateHeader accepts the header, so this is the seed that
// proves the harness gets into the deserializer.
TEST_F(OverlayFuzzSeedTest, validHeaderOneEntryDirParses) {
  auto result = fixture_.loadOverlayDir(
      ino_, asRange(fixture_.validDirHeader() + serializeOneEntryDir()));
  EXPECT_EQ(OverlayLoadOutcome::Parsed, result.outcome);
  EXPECT_EQ(1u, result.entryCount);
}

// The same seed through the hand-rolled compact-protocol reader.
TEST_F(OverlayFuzzSeedTest, validHeaderOneEntryDirParsesAsEntries) {
  auto result = fixture_.loadOverlayEntries(
      ino_, asRange(fixture_.validDirHeader() + serializeOneEntryDir()));
  EXPECT_EQ(OverlayLoadOutcome::Parsed, result.outcome);
  EXPECT_EQ(1u, result.declaredCount);
  EXPECT_EQ(1u, result.visitedCount);
}

// Seed: wrong identifier. "OVFL" is the file header, valid but not a dir.
TEST_F(OverlayFuzzSeedTest, wrongIdentifierIsRejected) {
  auto file = fixture_.validDirHeader() + serializeEmptyDir();
  file.replace(0, 4, FsFileContentStore::kHeaderIdentifierFile.str());
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(file)).outcome);
}

// Seed: wrong version.
TEST_F(OverlayFuzzSeedTest, wrongVersionIsRejected) {
  auto file = fixture_.validDirHeader() + serializeEmptyDir();
  file[7] = '\x02';
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(file)).outcome);
}

// Seed: header truncated below kHeaderLength.
TEST_F(OverlayFuzzSeedTest, truncatedHeaderIsRejected) {
  auto file = fixture_.validDirHeader().substr(0, 32);
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(file)).outcome);
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(std::string{})).outcome);
}

// Seed: valid header, payload truncated mid-struct.
TEST_F(OverlayFuzzSeedTest, truncatedPayloadIsRejected) {
  auto payload = serializeOneEntryDir();
  ASSERT_GT(payload.size(), 2u);
  auto file = fixture_.validDirHeader() + payload.substr(0, payload.size() - 2);
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(file)).outcome);
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayEntries(ino_, asRange(file)).outcome);
}

// Random bytes never get past the identifier check, which is what makes the
// seed corpus load-bearing for the whole-file vector.
TEST_F(OverlayFuzzSeedTest, randomBytesAreRejected) {
  std::string file(128, '\x41');
  EXPECT_EQ(
      OverlayLoadOutcome::Rejected,
      fixture_.loadOverlayDir(ino_, asRange(file)).outcome);
}

} // namespace
