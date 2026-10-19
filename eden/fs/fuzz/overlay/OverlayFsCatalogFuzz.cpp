/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cstdint>
#include <string>
#include <vector>

#include <folly/Range.h>

#include "eden/fs/fuzz/overlay/OverlayFileFuzzFixture.h"
#include "security/lionhead/utils/lib_ftest/ftest.h"

/**
 * Build the overlay directory once, before the first iteration, so the ~520
 * mkdir syscalls initialize() performs are not charged to a fuzz iteration.
 */
extern "C" int LionheadFuzzerCustomInitialize(int*, char***) {
  facebook::eden::OverlayFileFuzzFixture::global();
  return 0;
}

namespace {

using namespace facebook::eden;

folly::ByteRange asRange(const std::vector<uint8_t>& bytes) {
  return folly::ByteRange{bytes.data(), bytes.size()};
}

folly::ByteRange asRange(const std::string& bytes) {
  return folly::ByteRange{
      reinterpret_cast<const uint8_t*>(bytes.data()), bytes.size()};
}

std::string headerPlusPayload(
    const OverlayFileFuzzFixture& fixture,
    const std::vector<uint8_t>& payload) {
  std::string file = fixture.validDirHeader();
  file.append(reinterpret_cast<const char*>(payload.data()), payload.size());
  return file;
}

/**
 * The full on-disk overlay file is the fuzzer's remaining input, verbatim:
 * the 64-byte header (identifier, version, and the zero-filled remainder) and
 * the compact-protocol payload behind it, at whatever length the input has.
 *
 * This is the vector that exercises FsFileContentStore::validateHeader, which
 * is why the seed corpus matters — random bytes stop at the identifier check
 * and never reach the deserializer.
 */
FUZZ(OverlayFsCatalog, load_overlay_dir_file) {
  auto& fixture = OverlayFileFuzzFixture::global();
  auto ino = OverlayFileFuzzFixture::pickInode(f.u8("inode_selector"));
  auto contents = f.all_remaining_bytes();
  fixture.loadOverlayDir(ino, asRange(contents));
}

/**
 * A header that production actually wrote, followed by a fuzzed payload, so
 * every iteration gets past validateHeader. The same bytes then go through
 * both decoders of the format: CompactSerializer::deserialize<OverlayDir>
 * (loadOverlayDir) and the hand-written CompactProtocolReader decoder
 * (loadOverlayEntries), so the two are exercised on identical input. Each load
 * writes and unlinks its own copy of the file.
 */
FUZZ(OverlayFsCatalog, load_overlay_dir_payload) {
  auto& fixture = OverlayFileFuzzFixture::global();
  auto ino = OverlayFileFuzzFixture::pickInode(f.u8("inode_selector"));
  auto payload = f.all_remaining_bytes();
  auto file = headerPlusPayload(fixture, payload);
  fixture.loadOverlayDir(ino, asRange(file));
  fixture.loadOverlayEntries(ino, asRange(file));
}

} // namespace
