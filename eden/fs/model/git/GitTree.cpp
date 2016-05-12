/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GitTree.h"
#include <folly/Format.h>
#include <folly/String.h>
#include <git2/oid.h>
#include <array>
#include <string>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"

using folly::StringPiece;
using std::array;
using std::invalid_argument;
using std::string;
using std::vector;

namespace facebook {
namespace eden {

const int RWX = 0b111;
const int RW_ = 0b110;

enum GitModeMask {
  DIRECTORY = 040000,
  GIT_LINK = 0160000,
  REGULAR_EXECUTABLE_FILE = 0100755,
  REGULAR_FILE = 0100644,
  SYMLINK = 0120000,
};

std::unique_ptr<Tree> deserializeGitTree(
    const Hash& hash,
    StringPiece gitTreeObject) {
  // Find the end of the header and extract the size.
  constexpr StringPiece prefix("tree ");
  if (!gitTreeObject.startsWith(prefix)) {
    throw invalid_argument("Contents did not start with expected header.");
  }
  gitTreeObject.advance(prefix.size());

  auto contentSize = folly::to<unsigned int>(&gitTreeObject);
  if (gitTreeObject.at(0) != '\0') {
    throw invalid_argument("Header should be followed by NUL.");
  }
  gitTreeObject.advance(1);
  if (contentSize != gitTreeObject.size()) {
    throw invalid_argument("Size in header should match contents");
  }

  // Scan the gitTreeObject string and populate entries, as appropriate.
  vector<TreeEntry> entries;
  while (gitTreeObject.size() > 0) {
    // Extract the mode.
    auto modeEnd = gitTreeObject.find_first_of(' ');
    if (modeEnd == StringPiece::npos) {
      throw invalid_argument("Could not find space to delimit end of mode.");
    }

    auto modeStr = gitTreeObject.subpiece(0, modeEnd);
    size_t modeEndIndex;
    auto mode = std::stoi(modeStr.str(), &modeEndIndex, /* base */ 8);
    if (modeEndIndex != modeEnd) {
      throw invalid_argument("Did not parse expected number of octal chars.");
    }
    gitTreeObject.advance(modeEndIndex + 1); // +1 for space delimiter.

    // Extract the name.
    auto nameEndIndex = gitTreeObject.find_first_of('\0');
    if (nameEndIndex == StringPiece::npos) {
      throw invalid_argument("Could not find NUL to terminate name.");
    }
    auto name = gitTreeObject.subpiece(0, nameEndIndex);
    gitTreeObject.advance(nameEndIndex + 1); // +1 for NUL delimiter.

    // Extract the hash.
    if (gitTreeObject.size() < GIT_OID_RAWSZ) {
      throw invalid_argument(
          "Tree object does not have enough remaining room for hash.");
    }
    array<uint8_t, GIT_OID_RAWSZ> hashBytes;
    std::copy(
        gitTreeObject.begin(),
        gitTreeObject.begin() + GIT_OID_RAWSZ,
        hashBytes.data());
    gitTreeObject.advance(GIT_OID_RAWSZ);
    Hash entryHash(hashBytes);

    // Determine the individual fields from the mode.
    FileType fileType;
    uint8_t ownerPermissions;
    if (mode == GitModeMask::DIRECTORY) {
      fileType = FileType::DIRECTORY;
      ownerPermissions = RWX;
    } else if (mode == GitModeMask::REGULAR_FILE) {
      fileType = FileType::REGULAR_FILE;
      ownerPermissions = RW_;
    } else if (mode == GitModeMask::REGULAR_EXECUTABLE_FILE) {
      fileType = FileType::REGULAR_FILE;
      ownerPermissions = RWX;
    } else if (mode == GitModeMask::SYMLINK) {
      fileType = FileType::SYMLINK;
      ownerPermissions = RWX;
    } else if (mode == GitModeMask::GIT_LINK) {
      throw std::domain_error(folly::sformat(
          "Gitlinks are not currently supported: {:o} in object {}",
          static_cast<int>(mode),
          hash.toString()));
    } else {
      throw invalid_argument(folly::sformat(
          "Unrecognized mode: {:o} in object {}",
          static_cast<int>(mode),
          hash.toString()));
    }

    entries.emplace_back(entryHash, name, fileType, ownerPermissions);
  }

  return std::make_unique<Tree>(hash, std::move(entries));
}
}
}
