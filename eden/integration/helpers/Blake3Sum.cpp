/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cstdio>
#include <filesystem>
#include <iostream>
#include <sstream>

#include <folly/File.h>
#include <folly/String.h>
#include <folly/init/Init.h>
#include <folly/portability/GFlags.h>
#include <folly/system/MemoryMapping.h>

#include "eden/fs/digest/Blake3.h"

using namespace facebook::eden;

DEFINE_string(file, "", "A file for which to compute the digest");

DEFINE_string(key, "", "Blake3 key to use");

int main(int argc, char** argv) {
  const folly::Init init(&argc, &argv);

  auto blake3 = Blake3::create(
      FLAGS_key.empty() ? std::nullopt
                        : std::make_optional<std::string>(FLAGS_key));
  if (FLAGS_file.empty()) {
    constexpr size_t kBufSize = 1024;
    std::string buf;
    buf.reserve(kBufSize);
    std::stringstream ss;
    do {
      std::cin.read(buf.data(), kBufSize);
      std::string_view sv{buf.data(), (size_t)std::cin.gcount()};
      ss << sv;
    } while (!std::cin.eof());

    auto input = ss.str();
    size_t index = 0;
    while ((index = input.find("\\n", index)) != std::string::npos) {
      input.replace(index, 2, "\n");
      ++index;
    }

    blake3.update(input.data(), input.size());
  } else {
    auto fileExpected = folly::File::makeFile(FLAGS_file);
    if (fileExpected.hasError()) {
      std::cout << "Failed to open file " << FLAGS_file << ": "
                << fileExpected.error().what();
      return 1;
    }

    folly::MemoryMapping mmap(std::move(fileExpected).value());
    mmap.hintLinearScan();
    auto range = mmap.range();
    constexpr size_t kBlockSize = 8192;
    while (!range.empty()) {
      const auto piece = range.subpiece(0, kBlockSize);
      blake3.update(piece.data(), piece.size());
      range.advance(piece.size());
    }
  }

  std::array<uint8_t, 32> hash{};
  blake3.finalize(folly::MutableByteRange{hash.data(), hash.size()});
  std::cout << folly::hexlify(folly::ByteRange{hash.data(), hash.size()})
            << std::endl;
  return 0;
}
