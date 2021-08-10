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

class PrjfsDispatcher {
 public:
  virtual ~PrjfsDispatcher();
  explicit PrjfsDispatcher(EdenStats* stats);

  EdenStats* getStats() const;

  /**
   * Open a directory
   */
  virtual folly::Future<std::vector<FileMetadata>> opendir(
      RelativePath path,
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
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file was created
   */
  virtual folly::Future<folly::Unit> fileCreated(
      RelativePath path,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a directory was created
   */
  virtual folly::Future<folly::Unit> dirCreated(
      RelativePath path,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file has been modified
   */
  virtual folly::Future<folly::Unit> fileModified(
      RelativePath relPath,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file is renamed
   */
  virtual folly::Future<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a file was removed
   */
  virtual folly::Future<folly::Unit> fileDeleted(
      RelativePath relPath,
      ObjectFetchContext& context) = 0;

  /**
   * Notification sent when a directory was removed
   */
  virtual folly::Future<folly::Unit> dirDeleted(
      RelativePath relPath,
      ObjectFetchContext& context) = 0;

 private:
  EdenStats* stats_{nullptr};
};
} // namespace facebook::eden
