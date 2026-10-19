/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Writes the seed corpus for the eden/fs/fuzz/overlay harnesses.
 *
 * Random bytes fail FsFileContentStore::validateHeader on the identifier
 * check and never reach the Thrift deserializer, so the whole-file harness
 * only gets useful coverage from seeds that already carry a well-formed
 * header. The seeds live here as code rather than as checked-in blobs so
 * they stay tied to the format the catalog actually writes.
 *
 * A seed file is exactly what the FDP consumes: one byte for the
 * "inode_selector" draw, then all_remaining_bytes() verbatim. For the
 * file/ seeds those remaining bytes are the whole overlay file; for the
 * payload/ seeds they are only the compact-protocol payload, because those
 * harnesses prepend the header themselves.
 *
 *   buck2 run fbcode//eden/fs/fuzz/overlay:overlay_fuzz_seed_corpus -- <dir>
 */

#include <stdexcept>
#include <string>

#include <fmt/core.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/init/Init.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/fuzz/overlay/OverlayFileFuzzFixture.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"

using namespace facebook::eden;
using apache::thrift::CompactSerializer;

namespace {

constexpr char kInodeSelector = '\x07';

std::string emptyDirPayload() {
  return CompactSerializer::serialize<std::string>(overlay::OverlayDir{});
}

std::string oneEntryDirPayload() {
  overlay::OverlayDir dir;
  overlay::OverlayEntry entry;
  entry.mode() = 0100644;
  entry.inodeNumber() = 42;
  entry.isRestricted() = false;
  dir.entries()->emplace("hello", std::move(entry));
  return CompactSerializer::serialize<std::string>(dir);
}

std::string manyEntryDirPayload() {
  overlay::OverlayDir dir;
  for (int i = 0; i < 8; ++i) {
    overlay::OverlayEntry entry;
    entry.mode() = 040755;
    entry.inodeNumber() = 1000 + i;
    entry.hash() = std::string(20, static_cast<char>('a' + i));
    entry.isRestricted() = (i % 2) == 0;
    entry.aclRootState() = i % 3;
    dir.entries()->emplace(fmt::format("child{}", i), std::move(entry));
  }
  return CompactSerializer::serialize<std::string>(dir);
}

void writeSeed(
    AbsolutePathPiece dir,
    folly::StringPiece name,
    const std::string& fdpInput) {
  auto path = dir + PathComponentPiece{name};
  if (!folly::writeFile(fdpInput, path.c_str())) {
    throw std::runtime_error(fmt::format("failed to write {}", path));
  }
  fmt::print("{} ({} bytes)\n", path, fdpInput.size());
}

} // namespace

int main(int argc, char** argv) {
  auto init = folly::Init(&argc, &argv);
  if (argc < 2) {
    fmt::print(stderr, "usage: {} <output-dir>\n", argv[0]);
    return 1;
  }

  OverlayFileFuzzFixture fixture;
  const std::string& header = fixture.validDirHeader();
  const std::string empty = emptyDirPayload();
  const std::string one = oneEntryDirPayload();
  const std::string many = manyEntryDirPayload();

  auto root = canonicalPath(argv[1]);
  auto fileDir = root + PathComponentPiece{"file"};
  auto payloadDir = root + PathComponentPiece{"payload"};
  ensureDirectoryExists(fileDir);
  ensureDirectoryExists(payloadDir);

  const std::string prefix{kInodeSelector};

  // Whole-file seeds: header validation plus deserialization.
  writeSeed(fileDir, "valid_empty_dir", prefix + header + empty);
  writeSeed(fileDir, "valid_one_entry_dir", prefix + header + one);
  writeSeed(fileDir, "valid_many_entry_dir", prefix + header + many);

  auto wrongIdentifier = header + empty;
  wrongIdentifier.replace(
      0, 4, FsFileContentStore::kHeaderIdentifierFile.str());
  writeSeed(fileDir, "wrong_identifier", prefix + wrongIdentifier);

  auto wrongVersion = header + empty;
  wrongVersion[7] = '\x02';
  writeSeed(fileDir, "wrong_version", prefix + wrongVersion);

  writeSeed(fileDir, "truncated_header", prefix + header.substr(0, 32));
  writeSeed(fileDir, "empty_file", prefix);
  writeSeed(
      fileDir,
      "truncated_payload",
      prefix + header + one.substr(0, one.size() - 2));

  // Payload-only seeds: the harnesses supply the header.
  writeSeed(payloadDir, "empty_dir", prefix + empty);
  writeSeed(payloadDir, "one_entry_dir", prefix + one);
  writeSeed(payloadDir, "many_entry_dir", prefix + many);
  writeSeed(
      payloadDir, "truncated_dir", prefix + one.substr(0, one.size() - 2));
  writeSeed(payloadDir, "empty_payload", prefix);

  return 0;
}
