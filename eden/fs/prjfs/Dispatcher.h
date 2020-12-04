/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h> // @manual
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <class T>
class Future;
} // namespace folly

namespace facebook::eden {

class ObjectFetchContext;
class EdenStats;

struct InodeMetadata {
  // To ensure that the OS has a record of the canonical file name, and not
  // just whatever case was used to lookup the file, we capture the
  // relative path here.
  RelativePath path;
  size_t size;
  bool isDir;
};

struct LookupResult {
  InodeMetadata meta;
  std::function<void()> incFsRefcount;
};

class Dispatcher {
 public:
  virtual ~Dispatcher();
  explicit Dispatcher(EdenStats* stats);

  EdenStats* getStats() const;

  /**
   * Open a directory
   */
  virtual folly::Future<std::vector<FileMetadata>> opendir(
      RelativePathPiece path,
      ObjectFetchContext& context) = 0;

  /**
   * Lookup the specified file and get its attributes.
   */
  virtual folly::Future<std::optional<LookupResult>> lookup(
      RelativePath path,
      ObjectFetchContext& context) = 0;

  /**
   * Test if a file with the given name exist
   */
  virtual folly::Future<bool> access(
      RelativePath path,
      ObjectFetchContext& context) = 0;

  /**
   * Read the file with the given name
   *
   * Returns the entire content of the file at path.
   *
   * In the future, this will return only what's in between offset and
   * offset+length.
   */
  virtual folly::Future<std::string> read(
      RelativePath path,
      uint64_t offset,
      uint32_t length,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file was created
   */
  virtual folly::Future<folly::Unit> newFileCreated(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file was ovewritten
   */
  virtual folly::Future<folly::Unit> fileOverwritten(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file is closed after being modified
   */
  virtual folly::Future<folly::Unit> fileHandleClosedFileModified(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file is renamed
   */
  virtual folly::Future<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent prior to renaming a file
   *
   * A failure will block the rename operation
   */
  virtual folly::Future<folly::Unit> preRename(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file is being removed
   */
  virtual folly::Future<folly::Unit> fileHandleClosedFileDeleted(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent prior to creating a hardlink
   *
   * A failure will block the hardlink operation
   */
  virtual folly::Future<folly::Unit> preSetHardlink(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) = 0;

 private:
  EdenStats* stats_{nullptr};
};
} // namespace facebook::eden
