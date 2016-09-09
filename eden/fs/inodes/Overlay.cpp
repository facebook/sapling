/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Overlay.h"
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/inodes/gen-cpp2/overlay_types.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using folly::File;
using folly::StringPiece;
using folly::fbstring;
using folly::fbvector;
using apache::thrift::CompactSerializer;

/* Relative to the localDir, the metaFile holds the serialized rendition
 * of the overlay_ data.  We use thrift CompactSerialization for this.
 */
constexpr StringPiece kMetaDir{"overlay"};
constexpr StringPiece kMetaFile{"dirdata"};

/* Relative to the localDir, the overlay tree is where we create the
 * materialized directory structure; directories and files are created
 * here. */
constexpr StringPiece kOverlayTree{"tree"};

Overlay::Overlay(AbsolutePathPiece localDir)
    : localDir_(localDir),
      contentDir_(localDir + PathComponentPiece(kOverlayTree)) {
  auto res = mkdir(contentDir_.c_str(), 0700);
  if (res == -1 && errno != EEXIST) {
    folly::throwSystemError("mkdir: ", contentDir_);
  }
}

folly::Optional<TreeInode::Dir> Overlay::loadOverlayDir(
    RelativePathPiece path) const {
  auto metaFile = localDir_ + PathComponentPiece(kMetaDir) + path +
      PathComponentPiece(kMetaFile);

  // read metaFile and de-serialize into data
  std::string serializedData;
  if (!folly::readFile(metaFile.c_str(), serializedData)) {
    int err = errno;
    if (err == ENOENT) {
      // There is no overlay here
      return folly::none;
    }
    folly::throwSystemErrorExplicit(err, "failed to read ", metaFile);
  }
  auto dir =
      CompactSerializer::deserialize<overlay::OverlayDir>(serializedData);

  TreeInode::Dir result;

  // The treeHash, if present, identifies the Tree from which this directory
  // was derived.
  if (!dir.treeHash.empty()) {
    // We can't go direct to ByteRange from a std::string without some
    // nasty casting, so we're taking it to StringPiece then ByteRange.
    result.treeHash = Hash(folly::ByteRange(folly::StringPiece(dir.treeHash)));
  }

  for (auto& iter : dir.entries) {
    const auto& name = iter.first;
    const auto& value = iter.second;

    auto entry = std::make_unique<TreeInode::Entry>();
    entry->mode = value.mode;
    entry->materialized = value.materialized;
    if (!value.hash.empty()) {
      entry->hash = Hash(folly::ByteRange(folly::StringPiece(value.hash)));
    }
    result.entries.emplace(PathComponentPiece(name), std::move(entry));
  }

  return folly::Optional<TreeInode::Dir>(std::move(result));
}

void Overlay::saveOverlayDir(RelativePathPiece path, const TreeInode::Dir* dir)
    const {
  // Translate the data to the thrift equivalents
  overlay::OverlayDir odir;

  if (dir->treeHash) {
    auto bytes = dir->treeHash->getBytes();
    odir.treeHash =
        std::string(reinterpret_cast<const char*>(bytes.data()), bytes.size());
  }
  for (auto& entIter : dir->entries) {
    const auto& entName = entIter.first;
    const auto ent = entIter.second.get();

    overlay::OverlayEntry oent;
    oent.mode = ent->mode;
    oent.materialized = ent->materialized;
    if (ent->hash) {
      auto bytes = ent->hash->getBytes();
      oent.hash = std::string(
          reinterpret_cast<const char*>(bytes.data()), bytes.size());
    }

    odir.entries.emplace(
        std::make_pair(entName.stringPiece().str(), std::move(oent)));
  }

  // Ask thrift to serialize it.
  auto serializedData = CompactSerializer::serialize<std::string>(odir);

  // Now replace the file contents.  We do this by writing out to a temporary
  // file, then atomically swapping it with the file on disk.  This ensures that
  // we never leave the metaFile with partially written contents.

  // Compute the path to the dir where we're going to store this data
  auto metaPath = localDir_ + PathComponentPiece(kMetaDir) + path;
  auto result = ::mkdir(metaPath.c_str(), 0755);
  if (result != 0 && errno != EEXIST) {
    folly::throwSystemError("failed to mkdir ", metaPath);
  }

  // and the path to the file that we're going to store in here
  auto metaFile = metaPath + PathComponentPiece(kMetaFile);

  // First generate a uniquely named file.
  std::string tempName = metaFile.stringPiece().str() + "XXXXXX";
  SCOPE_EXIT {
    unlink(tempName.c_str());
  };
  auto fd = mkostemp(&tempName[0], O_CLOEXEC);
  if (fd == -1) {
    folly::throwSystemError("failed to open a temporary file");
  }
  folly::File file(fd, true);

  // Write the data to it.
  auto wrote =
      folly::writeFull(file.fd(), serializedData.data(), serializedData.size());
  int err = errno;
  if (wrote != serializedData.size()) {
    folly::throwSystemErrorExplicit(err, "failed to write to ", tempName);
  }
  file.close();

  // And lastly rename it into the right place.
  folly::checkUnixError(
      ::rename(tempName.c_str(), metaFile.c_str()),
      "failed to rename ",
      tempName,
      " to ",
      metaFile);
}

void Overlay::removeOverlayDir(RelativePathPiece path) const {
  auto metaPath = localDir_ + PathComponentPiece(kMetaDir) + path;
  auto metaFile = metaPath + PathComponentPiece(kMetaFile);

  folly::checkUnixError(::unlink(metaFile.c_str()), "unlink: ", metaFile);
  folly::checkUnixError(::rmdir(metaPath.c_str()), "rmdir: ", metaPath);
}

const AbsolutePath& Overlay::getLocalDir() const {
  return localDir_;
}

const AbsolutePath& Overlay::getContentDir() const {
  return contentDir_;
}
}
}
