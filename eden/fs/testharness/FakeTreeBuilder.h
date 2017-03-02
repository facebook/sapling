/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/ExceptionWrapper.h>
#include <folly/Range.h>
#include <memory>
#include "eden/fs/model/TreeEntry.h"
#include "eden/utils/PathFuncs.h"
#include "eden/utils/PathMap.h"

namespace facebook {
namespace eden {

class Blob;
class FakeBackingStore;
class Tree;

template <typename T>
class StoredObject;
using StoredTree = StoredObject<Tree>;
using StoredBlob = StoredObject<Blob>;

/**
 * FakeTreeBuilder is a helper class for populating Trees and Blobs in a
 * FakeBackingStore.
 *
 * FakeTreeBuilder provides APIs for defining the file structure.  The
 * finalize() method then turns this into Tree and Blob objects in the
 * FakeBackingStore.
 *
 * This class is not thread-safe.  Callers are responsible for performing
 * synchronization, if necessary.  (Typically this class is used only in a
 * single thread when building up the backing store data to use in a test.)
 */
class FakeTreeBuilder {
 public:
  class FileInfo;

  FakeTreeBuilder();

  FakeTreeBuilder(FakeTreeBuilder&&) = default;
  FakeTreeBuilder& operator=(FakeTreeBuilder&&) = default;

  /**
   * Create a new FakeTreeBuilder that starts with the same contents as this
   * FakeTreeBuilder.
   *
   * clone() can be called even on a finalized FakeTreeBuilder.
   *
   * This is useful for emulating a normal source control modification
   * workflow.  You can use separate FakeTreeBuilder objects for each commit
   * you want to create.  After you finalize one FakeTreeBuilder to create a
   * commit's root tree, you can clone it to get a new FakeTreeBuilder that you
   * can modify to create the root tree for another commit.
   */
  FakeTreeBuilder clone() const;

  /**
   * Define a file at the specified path.
   */
  void setFile(
      folly::StringPiece path,
      folly::StringPiece contents,
      int permissions = 0644) {
    setFile(RelativePathPiece{path}, folly::ByteRange{contents}, permissions);
  }
  void setFile(
      folly::StringPiece path,
      folly::ByteRange contents,
      int permissions = 0644) {
    setFile(RelativePathPiece{path}, contents, permissions);
  }
  void setFile(
      RelativePathPiece path,
      folly::StringPiece contents,
      int permissions = 0644) {
    setFile(path, folly::ByteRange{contents}, permissions);
  }
  void setFile(
      RelativePathPiece path,
      folly::ByteRange contents,
      int permissions = 0644) {
    setFileImpl(path, contents, false, FileType::REGULAR_FILE, permissions);
  }

  void setFiles(const std::initializer_list<FileInfo>& fileArgs);

  /**
   * Replace the contents of a file at the given path.
   */
  void replaceFile(
      folly::StringPiece path,
      folly::StringPiece contents,
      int permissions = 0644) {
    replaceFile(
        RelativePathPiece{path}, folly::ByteRange{contents}, permissions);
  }
  void replaceFile(
      folly::StringPiece path,
      folly::ByteRange contents,
      int permissions = 0644) {
    replaceFile(RelativePathPiece{path}, contents, permissions);
  }
  void replaceFile(
      RelativePathPiece path,
      folly::StringPiece contents,
      int permissions = 0644) {
    replaceFile(path, folly::ByteRange{contents}, permissions);
  }
  void replaceFile(
      RelativePathPiece path,
      folly::ByteRange contents,
      int permissions = 0644) {
    setFileImpl(path, contents, true, FileType::REGULAR_FILE, permissions);
  }

  /**
   * Define a symlink at the specified path.
   */
  void setSymlink(folly::StringPiece path, folly::StringPiece contents) {
    setSymlink(RelativePathPiece{path}, contents);
  }
  void setSymlink(RelativePathPiece path, folly::StringPiece contents) {
    setFileImpl(path, contents, false, FileType::SYMLINK, 0644);
  }

  /**
   * Replace any existing file at the given path with a symlink.
   */
  void replaceSymlink(folly::StringPiece path, folly::StringPiece contents) {
    replaceSymlink(RelativePathPiece{path}, contents);
  }
  void replaceSymlink(RelativePathPiece path, folly::StringPiece contents) {
    setFileImpl(path, contents, true, FileType::SYMLINK, 0644);
  }

  /**
   * Remove a file or symlink at the given path.
   */
  void removeFile(folly::StringPiece path) {
    removeFile(RelativePathPiece{path});
  }
  void removeFile(RelativePathPiece path);

  /**
   * Make sure a directory exists at the given path.
   *
   * This allows creating empty Tree objects in the backing store.
   * This does not generally happen in practice, but is potentially useful to
   * be able to do during testing.
   */
  void mkdir(folly::StringPiece path) {
    mkdir(RelativePathPiece{path});
  }
  void mkdir(RelativePathPiece path);

  /**
   * Call setReady() on the StoredTree or StoredBlob at the given path.
   */
  void setReady(folly::StringPiece path) {
    setReady(RelativePathPiece{path});
  }
  void setReady(RelativePathPiece path);

  /**
   * Call triggerError() on the StoredTree or StoredBlob at the given path.
   */
  template <class E>
  void triggerError(RelativePathPiece path, const E& e) {
    triggerError(path, folly::make_exception_wrapper<E>(e));
  }
  template <class E>
  void triggerError(folly::StringPiece path, const E& e) {
    triggerError(RelativePathPiece{path}, folly::make_exception_wrapper<E>(e));
  }
  void triggerError(RelativePathPiece path, folly::exception_wrapper ew);
  void triggerError(folly::StringPiece path, folly::exception_wrapper ew) {
    triggerError(RelativePathPiece{path}, std::move(ew));
  }

  /**
   * Update the FakeBackingStore with Tree and Blob objects from this
   * FakeTreeBuilder's data.
   *
   * Call this to populate the store after calling setFile(), replaceFile(),
   * and other similar APIs to set up the file state as desired.
   *
   * If setReady is true, the objects stored in the FakeBackingStore will be
   * marked as immediately ready.  This applies to new Trees and Blobs created
   * by finalize, and also to any existing Trees and Blobs found if parts of
   * the tree are identical to Trees and Blobs already present in the
   * FakeBackingStore.
   */
  StoredTree* finalize(std::shared_ptr<FakeBackingStore> store, bool setReady);

  StoredTree* getRoot() const;

 private:
  enum ExplicitClone { CLONE };
  FakeTreeBuilder(ExplicitClone, const FakeTreeBuilder* orig);

  struct EntryInfo {
    EntryInfo(FileType fileType, uint8_t perms);

    EntryInfo(EntryInfo&& other) = default;
    EntryInfo& operator=(EntryInfo&& other) = default;

    /**
     * Create a deep copy of an EntryInfo object
     */
    EntryInfo(ExplicitClone, const EntryInfo& orig);

    StoredTree* finalizeTree(FakeTreeBuilder* builder, bool setReady) const;
    StoredBlob* finalizeBlob(FakeTreeBuilder* builder, bool setReady) const;

    FileType type;
    uint8_t ownerPermissions;
    std::unique_ptr<PathMap<EntryInfo>> entries;
    std::string contents;
  };

  FakeTreeBuilder(FakeTreeBuilder const&) = delete;
  FakeTreeBuilder& operator=(FakeTreeBuilder const&) = delete;

  void setFileImpl(
      RelativePathPiece path,
      folly::ByteRange contents,
      bool replace,
      FileType type,
      int permissions);
  EntryInfo* getEntry(RelativePathPiece path);
  EntryInfo* getDirEntry(RelativePathPiece path, bool create);
  StoredTree* getStoredTree(RelativePathPiece path);

  std::shared_ptr<FakeBackingStore> store_{nullptr};
  EntryInfo root_{FileType::DIRECTORY, 0b111};
  StoredTree* finalizedRoot_{nullptr};
};

struct FakeTreeBuilder::FileInfo {
  RelativePath path;
  std::string contents;
  int permissions = 0644;

  FileInfo(folly::StringPiece p, folly::StringPiece c, int perms = 0644)
      : path(p), contents(c.str()), permissions(perms) {}
};
}
}
