/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/config/CheckoutConfig.h"

#include <cpptoml.h> // @manual=fbsource//third-party/cpptoml:cpptoml

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/json.h>

#ifdef _WIN32
#include "eden/fs/win/utils/FileUtils.h" // @manual
#else
#include <folly/File.h>
#include <folly/FileUtil.h>
#endif

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using std::optional;
using std::string;

namespace {
// TOML config file for the individual client.
const facebook::eden::RelativePathPiece kCheckoutConfig{"config.toml"};

// Keys for the TOML config file.
constexpr folly::StringPiece kBindMountsSection{"bind-mounts"};
constexpr folly::StringPiece kRepoSection{"repository"};
constexpr folly::StringPiece kRepoSourceKey{"path"};
constexpr folly::StringPiece kRepoTypeKey{"type"};

// Files of interest in the client directory.
const facebook::eden::RelativePathPiece kSnapshotFile{"SNAPSHOT"};
const facebook::eden::RelativePathPiece kBindMountsDir{"bind-mounts"};
const facebook::eden::RelativePathPiece kOverlayDir{"local"};

// File holding mapping of client directories.
const facebook::eden::RelativePathPiece kClientDirectoryMap{"config.json"};

// Constants for use with the SNAPSHOT file
//
// The SNAPSHOT file format is:
// - 4 byte identifier: "eden"
// - 4 byte format version number (big endian)
// - 20 byte commit ID
// - (Optional 20 byte commit ID, only present when there are 2 parents)
constexpr folly::StringPiece kSnapshotFileMagic{"eden"};
enum : uint32_t {
  kSnapshotHeaderSize = 8,
  kSnapshotFormatVersion = 1,
};
} // namespace

namespace facebook {
namespace eden {

CheckoutConfig::CheckoutConfig(
    AbsolutePathPiece mountPath,
    AbsolutePathPiece clientDirectory)
    : clientDirectory_(clientDirectory), mountPath_(mountPath) {}

ParentCommits CheckoutConfig::getParentCommits() const {
  // Read the snapshot.
  auto snapshotFile = getSnapshotPath();
  string snapshotFileContents;

#ifdef _WIN32
  readFile(snapshotFile.c_str(), snapshotFileContents);
#else
  folly::readFile(snapshotFile.c_str(), snapshotFileContents);
#endif

  StringPiece contents{snapshotFileContents};
  if (!contents.startsWith(kSnapshotFileMagic)) {
    // Try reading an old-style SNAPSHOT file that just contains a single
    // commit ID, as an ASCII hexadecimal string.
    //
    // TODO: In the not-to-distant future we can remove support for this old
    // format, and simply throw an exception here if the snapshot file does not
    // start with the correct identifier bytes.
    auto snapshotID = folly::trimWhitespace(contents);
    return ParentCommits{Hash{snapshotID}};
  }

  if (contents.size() < kSnapshotHeaderSize) {
    throw std::runtime_error(folly::sformat(
        "eden SNAPSHOT file is too short ({} bytes): {}",
        contents.size(),
        snapshotFile));
  }

  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{contents});
  folly::io::Cursor cursor(&buf);
  cursor += kSnapshotFileMagic.size();
  auto version = cursor.readBE<uint32_t>();
  if (version != kSnapshotFormatVersion) {
    throw std::runtime_error(folly::sformat(
        "unsupported eden SNAPSHOT file format (version {}): {}",
        uint32_t{version},
        snapshotFile));
  }

  auto sizeLeft = cursor.length();
  if (sizeLeft != Hash::RAW_SIZE && sizeLeft != (Hash::RAW_SIZE * 2)) {
    throw std::runtime_error(folly::sformat(
        "unexpected length for eden SNAPSHOT file ({} bytes): {}",
        contents.size(),
        snapshotFile));
  }

  ParentCommits parents;
  cursor.pull(parents.parent1().mutableBytes().data(), Hash::RAW_SIZE);

  if (!cursor.isAtEnd()) {
    parents.parent2() = Hash{};
    cursor.pull(parents.parent2()->mutableBytes().data(), Hash::RAW_SIZE);
  }

  return parents;
}

void CheckoutConfig::setParentCommits(const ParentCommits& parents) const {
  std::array<uint8_t, kSnapshotHeaderSize + (2 * Hash::RAW_SIZE)> buffer;
  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{buffer});
  folly::io::RWPrivateCursor cursor{&buf};

  // Snapshot file format:
  // 4-byte identifier: "eden"
  cursor.push(ByteRange{kSnapshotFileMagic});
  // 4-byte format version identifier
  cursor.writeBE<uint32_t>(kSnapshotFormatVersion);
  // 20-byte commit ID: parent1
  cursor.push(parents.parent1().getBytes());
  // Optional 20-byte commit ID: parent2
  if (parents.parent2().has_value()) {
    cursor.push(parents.parent2()->getBytes());
    CHECK(cursor.isAtEnd());
  }
  size_t writtenSize = cursor - folly::io::RWPrivateCursor{&buf};
  ByteRange snapshotData{buffer.data(), writtenSize};
#ifdef _WIN32
  writeFileAtomic(getSnapshotPath().c_str(), snapshotData);
#else
  folly::writeFileAtomic(getSnapshotPath().stringPiece(), snapshotData);
#endif
}

void CheckoutConfig::setParentCommits(Hash parent1, std::optional<Hash> parent2)
    const {
  return setParentCommits(ParentCommits{parent1, parent2});
}

const AbsolutePath& CheckoutConfig::getClientDirectory() const {
  return clientDirectory_;
}

AbsolutePath CheckoutConfig::getSnapshotPath() const {
  return clientDirectory_ + kSnapshotFile;
}

AbsolutePath CheckoutConfig::getOverlayPath() const {
  return clientDirectory_ + kOverlayDir;
}

std::unique_ptr<CheckoutConfig> CheckoutConfig::loadFromClientDirectory(
    AbsolutePathPiece mountPath,
    AbsolutePathPiece clientDirectory) {
  // Extract repository name from the client config file
  auto configPath = clientDirectory + kCheckoutConfig;
  auto configRoot = cpptoml::parse_file(configPath.c_str());

  // Construct CheckoutConfig object
  auto config = std::make_unique<CheckoutConfig>(mountPath, clientDirectory);

  // Load repository information
  auto repository = configRoot->get_table(kRepoSection.str());
  config->repoType_ = *repository->get_as<std::string>(kRepoTypeKey.str());
  config->repoSource_ = *repository->get_as<std::string>(kRepoSourceKey.str());

  // Extract the bind mounts
  AbsolutePath bindMountsPath = clientDirectory + kBindMountsDir;
  auto bindMounts = configRoot->get_table(kBindMountsSection.str());
  if (bindMounts != nullptr) {
    for (auto& item : *bindMounts) {
      auto pathInClientDir = bindMountsPath + RelativePathPiece{item.first};
      auto tomlValue = item.second->as<std::string>();
      auto pathInMountDir =
          mountPath + RelativePathPiece{tomlValue.get()->get()};
      config->bindMounts_.emplace_back(pathInClientDir, pathInMountDir);
    }
  }

  return config;
}

folly::dynamic CheckoutConfig::loadClientDirectoryMap(
    AbsolutePathPiece edenDir) {
  // Extract the JSON and strip any comments.
  std::string jsonContents;
  auto configJsonFile = edenDir + kClientDirectoryMap;
#ifdef _WIN32
  readFile(configJsonFile.c_str(), jsonContents);
#else
  folly::readFile(configJsonFile.c_str(), jsonContents);
#endif
  auto jsonWithoutComments = folly::json::stripComments(jsonContents);
  if (jsonWithoutComments.empty()) {
    return folly::dynamic::object();
  }

  // Parse the comment-free JSON while tolerating trailing commas.
  folly::json::serialization_opts options;
  options.allow_trailing_comma = true;
  return folly::parseJson(jsonWithoutComments, options);
}
} // namespace eden
} // namespace facebook
