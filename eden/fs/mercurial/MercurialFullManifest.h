/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/utils/LeaseCache.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class LocalMercurialRepoAndRev;

// Maintains information about the monolithic hg manifest.
// During construction we use `hg files` to discover the directories.
// Later, we fill out basic stat(2) like information on demand.
// This doesn't perform too well with the hg of today, but is closer
// to the access pattern that we looking for.  I fully expect that we'll
// tear this up as we iterate.
class MercurialFullManifest {
 public:
  // For a directory within the manifest, the list of files and child dirs.
  // Both are sorted
  struct DirListing {
    folly::fbvector<folly::fbstring> files;
    folly::fbvector<folly::fbstring> dirs;
  };
  // For a file, the basic info we can use to fill out a `struct stat`
  struct FileInfo {
    mode_t mode;
    size_t size;

    FileInfo(mode_t mode, size_t size);
  };
  static std::unique_ptr<MercurialFullManifest> parseManifest(
      LocalMercurialRepoAndRev& repo);

  // Returns information about a given file.
  // This is backed by the LeaseCache
  folly::Future<std::shared_ptr<FileInfo>> getFileInfo(RelativePathPiece path);

  // Returns an object containing the list of entries for a given dir
  const DirListing& getListing(const folly::fbstring& path);

  // An optimization that can bulk load the FileInfo for a given dir
  folly::Future<folly::Unit> prefetchFileInfoForDir(RelativePathPiece path);

  // Obtain the contents of the specified path.
  // For symlinks this is the target of the symlink.
  // For plain files this is the content of the file itself.
  folly::Future<std::string> catFile(RelativePathPiece path);

 private:
  explicit MercurialFullManifest(LocalMercurialRepoAndRev& repo);
  LocalMercurialRepoAndRev& repo_;
  std::unordered_map<folly::fbstring, DirListing> dirs_;
  LeaseCache<folly::fbstring, FileInfo> fileInfo_;

  void load();
  DirListing& getOrMakeEntry(folly::StringPiece path);
  folly::Future<std::shared_ptr<FileInfo>> fetchFileInfo(
      const folly::fbstring& name);
};
}
}
