/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/CheckoutConfig.h"

#include <cpptoml.h>

#include <folly/Range.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/json.h>
#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/PathMap.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/Throw.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;

namespace facebook::eden {
namespace {
// TOML config file for the individual client.
const RelativePathPiece kCheckoutConfig{"config.toml"};

// Keys for the TOML config file.
constexpr folly::StringPiece kRepoSection{"repository"};
constexpr folly::StringPiece kRepoSourceKey{"path"};
constexpr folly::StringPiece kRepoTypeKey{"type"};
constexpr folly::StringPiece kRepoCaseSensitiveKey{"case-sensitive"};
constexpr folly::StringPiece kMountProtocol{"protocol"};
constexpr folly::StringPiece kRequireUtf8Path{"require-utf8-path"};
constexpr folly::StringPiece kEnableTreeOverlay{"enable-tree-overlay"};
constexpr folly::StringPiece kUseWriteBackCache{"use-write-back-cache"};
#ifdef _WIN32
constexpr folly::StringPiece kRepoGuid{"guid"};
#endif

// Files of interest in the client directory.
const RelativePathPiece kSnapshotFile{"SNAPSHOT"};
const RelativePathPiece kOverlayDir{"local"};

// File holding mapping of client directories.
const RelativePathPiece kClientDirectoryMap{"config.json"};

// Constants for use with the SNAPSHOT file
//
// - 4 byte identifier: "eden"
// - 4 byte format version number (big endian)
//
// Followed by:
// Version 1:
// - 20 byte commit ID
// - (Optional 20 byte commit ID, only present when there are 2 parents)
// Version 2:
// - 32-bit length
// - Arbitrary-length binary string of said length
// Version 3: (checkout in progress)
// - 32-bit pid of EdenFS process doing the checkout
// - 32-bit length
// - Arbitrary-length binary string of said length for the commit being updated
// from
// - 32-bit length
// - Arbitrary-length binary string of said length for the commit being updated
// to
// Version 4: (Working copy parent and checked out revision)
// - 32-bit length of working copy parent
// - Arbitrary-length binary string of said length for the working copy parent
// - 32-bit length of checked out revision
// - Arbitrary-length binary string of said length for the checked out revision
constexpr folly::StringPiece kSnapshotFileMagic{"eden"};
enum : uint32_t {
  kSnapshotHeaderSize = 8,
  // Legacy SNAPSHOT file version.
  kSnapshotFormatVersion1 = 1,
  // Legacy SNAPSHOT file version.
  kSnapshotFormatVersion2 = 2,
  // State of the SNAPSHOT file when a checkout operation is ongoing.
  kSnapshotFormatCheckoutInProgressVersion = 3,
  // State of the SNAPSHOT file when no checkout operation is ongoing. The
  // SNAPSHOT contains both the currently checked out RootId, as well as the
  // RootId most recently reset to.
  kSnapshotFormatWorkingCopyParentAndCheckedOutRevisionVersion = 4,
};
} // namespace

CheckoutConfig::CheckoutConfig(
    AbsolutePathPiece mountPath,
    AbsolutePathPiece clientDirectory)
    : clientDirectory_(clientDirectory), mountPath_(mountPath) {}

ParentCommit CheckoutConfig::getParentCommit() const {
  // Read the snapshot.
  auto snapshotFile = getSnapshotPath();
  auto snapshotFileContents = readFile(snapshotFile).value();

  StringPiece contents{snapshotFileContents};

  if (contents.size() < kSnapshotHeaderSize) {
    throwf<std::runtime_error>(
        "eden SNAPSHOT file is too short ({} bytes): {}",
        contents.size(),
        snapshotFile);
  }

  if (!contents.startsWith(kSnapshotFileMagic)) {
    throw std::runtime_error("unsupported legacy SNAPSHOT file");
  }

  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{contents});
  folly::io::Cursor cursor(&buf);
  cursor += kSnapshotFileMagic.size();
  auto version = cursor.readBE<uint32_t>();
  auto sizeLeft = cursor.length();
  switch (version) {
    case kSnapshotFormatVersion1: {
      if (sizeLeft != Hash20::RAW_SIZE && sizeLeft != (Hash20::RAW_SIZE * 2)) {
        throwf<std::runtime_error>(
            "unexpected length for eden SNAPSHOT file ({} bytes): {}",
            contents.size(),
            snapshotFile);
      }

      Hash20 parent1;
      cursor.pull(parent1.mutableBytes().data(), Hash20::RAW_SIZE);

      if (!cursor.isAtEnd()) {
        // This is never used by EdenFS.
        Hash20 secondParent;
        cursor.pull(secondParent.mutableBytes().data(), Hash20::RAW_SIZE);
      }

      auto rootId = RootId{parent1.toString()};

      // SNAPSHOT v1 stored hashes as binary, but RootId prefers them inflated
      // to human-readable ASCII, so hexlify here.
      return ParentCommit::WorkingCopyParentAndCheckedOutRevision{
          rootId, rootId};
    }

    case kSnapshotFormatVersion2: {
      auto bodyLength = cursor.readBE<uint32_t>();

      // The remainder of the file is the root ID.
      auto rootId = RootId{cursor.readFixedString(bodyLength)};

      return ParentCommit::WorkingCopyParentAndCheckedOutRevision{
          rootId, rootId};
    }

    case kSnapshotFormatCheckoutInProgressVersion: {
      auto pid = cursor.readBE<int32_t>();

      auto fromLength = cursor.readBE<uint32_t>();
      std::string fromRootId = cursor.readFixedString(fromLength);

      auto toLength = cursor.readBE<uint32_t>();
      std::string toRootId = cursor.readFixedString(toLength);

      return ParentCommit::CheckoutInProgress{
          RootId{std::move(fromRootId)}, RootId{std::move(toRootId)}, pid};
    }

    case kSnapshotFormatWorkingCopyParentAndCheckedOutRevisionVersion: {
      auto workingCopyParentLength = cursor.readBE<uint32_t>();
      auto workingCopyParent =
          RootId{cursor.readFixedString(workingCopyParentLength)};

      auto checkedOutLength = cursor.readBE<uint32_t>();
      auto checkedOutRootId = RootId{cursor.readFixedString(checkedOutLength)};

      return ParentCommit::WorkingCopyParentAndCheckedOutRevision{
          std::move(workingCopyParent), std::move(checkedOutRootId)};
    }

    default:
      throwf<std::runtime_error>(
          "unsupported eden SNAPSHOT file format (version {}): {}",
          uint32_t{version},
          snapshotFile);
  }
}

namespace {
void writeWorkingCopyParentAndCheckedOutRevisision(
    AbsolutePathPiece path,
    const RootId& workingCopy,
    const RootId& checkedOut) {
  const auto& workingCopyString = workingCopy.value();
  XCHECK_LE(workingCopyString.size(), std::numeric_limits<uint32_t>::max());

  const auto& checkedOutString = checkedOut.value();
  XCHECK_LE(checkedOutString.size(), std::numeric_limits<uint32_t>::max());

  auto buf = IOBuf::create(
      kSnapshotHeaderSize + 2 * sizeof(uint32_t) + workingCopyString.size() +
      checkedOutString.size());
  folly::io::Appender cursor{buf.get(), 0};

  // Snapshot file format:
  // 4-byte identifier: "eden"
  cursor.push(ByteRange{kSnapshotFileMagic});
  // 4-byte format version identifier
  cursor.writeBE<uint32_t>(
      kSnapshotFormatWorkingCopyParentAndCheckedOutRevisionVersion);

  // Working copy parent
  cursor.writeBE<uint32_t>(workingCopyString.size());
  cursor.push(folly::StringPiece{workingCopyString});

  // Checked out commit
  cursor.writeBE<uint32_t>(checkedOutString.size());
  cursor.push(folly::StringPiece{checkedOutString});

  writeFileAtomic(path, ByteRange{buf->data(), buf->length()}).value();
}
} // namespace

void CheckoutConfig::setCheckedOutCommit(const RootId& commit) const {
  // Pass the same commit for the working copy parent and the checked out
  // commit as a checkout sets both to the same value.
  writeWorkingCopyParentAndCheckedOutRevisision(
      getSnapshotPath(), commit, commit);
}

void CheckoutConfig::setWorkingCopyParentCommit(const RootId& commit) const {
  // The checked out commit doesn't change, re-use what's in the file currently
  auto parentCommit = getParentCommit();
  auto checkedOutRootId =
      parentCommit.getLastCheckoutId(ParentCommit::RootIdPreference::OnlyStable)
          .value();

  writeWorkingCopyParentAndCheckedOutRevisision(
      getSnapshotPath(), commit, checkedOutRootId);
}

void CheckoutConfig::setCheckoutInProgress(const RootId& from, const RootId& to)
    const {
  auto& fromString = from.value();
  auto& toString = to.value();

  auto buf = IOBuf::create(
      kSnapshotHeaderSize + 3 * sizeof(uint32_t) + fromString.size() +
      toString.size());
  folly::io::Appender cursor{buf.get(), 0};

  // Snapshot file format:
  // 4-byte identifier: "eden"
  cursor.push(ByteRange{kSnapshotFileMagic});
  // 4-byte format version identifier
  cursor.writeBE<uint32_t>(kSnapshotFormatCheckoutInProgressVersion);

  // PID of this process
  cursor.writeBE<uint32_t>(getpid());

  // From:
  cursor.writeBE<uint32_t>(fromString.size());
  cursor.push(folly::StringPiece{fromString});

  // To:
  cursor.writeBE<uint32_t>(toString.size());
  cursor.push(folly::StringPiece{toString});

  writeFileAtomic(getSnapshotPath(), ByteRange{buf->data(), buf->length()})
      .value();
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

  FieldConverter<MountProtocol> converter;
  MountProtocol mountProtocol = kMountProtocolDefault;
  auto mountProtocolStr = repository->get_as<std::string>(kMountProtocol.str());
  if (mountProtocolStr) {
    mountProtocol = converter.fromString(*mountProtocolStr, {})
                        .value_or(kMountProtocolDefault);
  }
  config->mountProtocol_ = mountProtocol;

  // Read optional case-sensitivity.
  auto caseSensitive = repository->get_as<bool>(kRepoCaseSensitiveKey.str());
  config->caseSensitive_ = caseSensitive
      ? static_cast<CaseSensitivity>(*caseSensitive)
      : kPathMapDefaultCaseSensitive;

  auto requireUtf8Path = repository->get_as<bool>(kRequireUtf8Path.str());
  config->requireUtf8Path_ = requireUtf8Path ? *requireUtf8Path : true;

  auto enableTreeOverlay = repository->get_as<bool>(kEnableTreeOverlay.str());
  // Treeoverlay is default on Windows
  config->enableTreeOverlay_ = enableTreeOverlay.value_or(folly::kIsWindows);

  auto useWriteBackCache = repository->get_as<bool>(kUseWriteBackCache.str());
  config->useWriteBackCache_ = useWriteBackCache.value_or(false);

#ifdef _WIN32
  auto guid = repository->get_as<std::string>(kRepoGuid.str());
  config->repoGuid_ = guid ? Guid{*guid} : Guid::generate();
#endif

  return config;
}

folly::dynamic CheckoutConfig::loadClientDirectoryMap(
    AbsolutePathPiece edenDir) {
  // Extract the JSON and strip any comments.
  auto configJsonFile = edenDir + kClientDirectoryMap;
  auto fileContents = readFile(configJsonFile);

  if (auto* exc = fileContents.tryGetExceptionObject<std::system_error>();
      exc && isEnoent(*exc)) {
    return folly::dynamic::object();
  }
  auto jsonContents = fileContents.value();
  auto jsonWithoutComments = folly::json::stripComments(jsonContents);
  if (jsonWithoutComments.empty()) {
    return folly::dynamic::object();
  }

  // Parse the comment-free JSON while tolerating trailing commas.
  folly::json::serialization_opts options;
  options.allow_trailing_comma = true;
  return folly::parseJson(jsonWithoutComments, options);
}

MountProtocol CheckoutConfig::getMountProtocol() const {
  // NFS is the only mount protocol that we allow to be switched from the
  // default.
  return mountProtocol_ == MountProtocol::NFS ? MountProtocol::NFS
                                              : kMountProtocolDefault;
}

} // namespace facebook::eden
