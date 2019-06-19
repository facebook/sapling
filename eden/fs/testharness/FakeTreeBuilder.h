/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/ExceptionWrapper.h>
#include <folly/Range.h>
#include <memory>
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/PathMap.h"

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
      bool executable = false) {
    setFile(RelativePathPiece{path}, folly::ByteRange{contents}, executable);
  }
  void setFile(
      folly::StringPiece path,
      folly::ByteRange contents,
      bool executable = false) {
    setFile(RelativePathPiece{path}, contents, executable);
  }
  void setFile(
      RelativePathPiece path,
      folly::StringPiece contents,
      bool executable = false) {
    setFile(path, folly::ByteRange{contents}, executable);
  }
  void setFile(
      RelativePathPiece path,
      folly::ByteRange contents,
      bool executable = false) {
    setFileImpl(
        path,
        contents,
        false,
        executable ? TreeEntryType::EXECUTABLE_FILE
                   : TreeEntryType::REGULAR_FILE);
  }

  void setFiles(const std::initializer_list<FileInfo>& fileArgs);

  /**
   * Replace the contents of a file at the given path.
   */
  void replaceFile(
      folly::StringPiece path,
      folly::StringPiece contents,
      bool executable = false) {
    replaceFile(
        RelativePathPiece{path}, folly::ByteRange{contents}, executable);
  }
  void replaceFile(
      folly::StringPiece path,
      folly::ByteRange contents,
      bool executable = false) {
    replaceFile(RelativePathPiece{path}, contents, executable);
  }
  void replaceFile(
      RelativePathPiece path,
      folly::StringPiece contents,
      bool executable = false) {
    replaceFile(path, folly::ByteRange{contents}, executable);
  }
  void replaceFile(
      RelativePathPiece path,
      folly::ByteRange contents,
      bool executable = false) {
    setFileImpl(
        path,
        contents,
        true,
        executable ? TreeEntryType::EXECUTABLE_FILE
                   : TreeEntryType::REGULAR_FILE);
  }

  /**
   * Define a symlink at the specified path.
   */
  void setSymlink(folly::StringPiece path, folly::StringPiece contents) {
    setSymlink(RelativePathPiece{path}, contents);
  }
  void setSymlink(RelativePathPiece path, folly::StringPiece contents) {
    setFileImpl(path, contents, false, TreeEntryType::SYMLINK);
  }

  /**
   * Replace any existing file at the given path with a symlink.
   */
  void replaceSymlink(folly::StringPiece path, folly::StringPiece contents) {
    replaceSymlink(RelativePathPiece{path}, contents);
  }
  void replaceSymlink(RelativePathPiece path, folly::StringPiece contents) {
    setFileImpl(path, contents, true, TreeEntryType::SYMLINK);
  }

  /**
   * Remove a file or symlink at the given path.
   */
  void removeFile(folly::StringPiece path, bool removeEmptyParents = true) {
    removeFile(RelativePathPiece{path}, removeEmptyParents);
  }
  void removeFile(RelativePathPiece path, bool removeEmptyParents = true);

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
   * Call setReady() on all Trees and Blobs used by this FakeTreeBuilder's root
   * Tree.
   *
   * Note that this will mark all Tree and Blob objects as ready if they are
   * referenced somehow by this builder's root Tree, even if they were already
   * present in the BackingStore when finalize() was called on this builder.
   */
  void setAllReady();

  /**
   * Call setReady() on all Trees and Blobs under the specified Tree.
   *
   * This also calls setReady() in the input Tree itself.
   */
  void setAllReadyUnderTree(StoredTree* tree);
  void setAllReadyUnderTree(RelativePathPiece path);
  void setAllReadyUnderTree(folly::StringPiece path) {
    setAllReadyUnderTree(RelativePathPiece{path});
  }

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

  /**
   * Get the StoredTree at the specified path.
   */
  StoredTree* getStoredTree(RelativePathPiece path);

  /**
   * Get the StoredBlob at the specified path.
   */
  StoredBlob* getStoredBlob(RelativePathPiece path);

 private:
  enum ExplicitClone { CLONE };
  FakeTreeBuilder(ExplicitClone, const FakeTreeBuilder* orig);

  struct EntryInfo {
    explicit EntryInfo(TreeEntryType fileType);

    EntryInfo(EntryInfo&& other) = default;
    EntryInfo& operator=(EntryInfo&& other) = default;

    /**
     * Create a deep copy of an EntryInfo object
     */
    EntryInfo(ExplicitClone, const EntryInfo& orig);

    StoredTree* finalizeTree(FakeTreeBuilder* builder, bool setReady) const;
    StoredBlob* finalizeBlob(FakeTreeBuilder* builder, bool setReady) const;

    TreeEntryType type;
    std::unique_ptr<PathMap<EntryInfo>> entries;
    std::string contents;
  };

  FakeTreeBuilder(FakeTreeBuilder const&) = delete;
  FakeTreeBuilder& operator=(FakeTreeBuilder const&) = delete;

  void setFileImpl(
      RelativePathPiece path,
      folly::ByteRange contents,
      bool replace,
      TreeEntryType type);
  EntryInfo* getEntry(RelativePathPiece path);
  EntryInfo* getDirEntry(RelativePathPiece path, bool create);

  std::shared_ptr<FakeBackingStore> store_{nullptr};
  EntryInfo root_{TreeEntryType::TREE};
  StoredTree* finalizedRoot_{nullptr};
};

class FakeTreeBuilder::FileInfo {
 public:
  RelativePath path;
  std::string contents;
  bool executable;

  FileInfo(folly::StringPiece p, folly::StringPiece c, bool exec = false)
      : path(p), contents(c.str()), executable(exec) {}
};
} // namespace eden
} // namespace facebook
