/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <vector>

namespace folly {
template <typename T>
class Future;
struct Unit;
} // namespace folly

namespace facebook {
namespace eden {

class Blob;
class BlobMetadata;
class Hash;
class Tree;

/**
 * ObjectStore calls methods on this context when fetching objects.
 * It's primarily used to track when and why source control objects are fetched.
 */
class ObjectFetchContext {
 public:
  /**
   * Which object type was fetched.
   *
   * Suitable for use as an index into an array of size kObjectTypeEnumMax
   */
  enum ObjectType {
    Blob,
    BlobMetadata,
    Tree,
    kObjectTypeEnumMax,
  };

  /**
   * Which cache satisfied a lookup request.
   *
   * Suitable for use as an index into an array of size kOriginEnumMax.
   */
  enum Origin {
    FromMemoryCache,
    FromDiskCache,
    FromBackingStore,
    kOriginEnumMax,
  };

  ObjectFetchContext() = default;
  virtual ~ObjectFetchContext() = default;
  virtual void didFetch(ObjectType, const Hash&, Origin) {}

  /**
   * Return a no-op fetch context suitable when no tracking is desired.
   */
  static ObjectFetchContext& getNullContext();

 private:
  ObjectFetchContext(const ObjectFetchContext&) = delete;
  ObjectFetchContext& operator=(const ObjectFetchContext&) = delete;
};

class IObjectStore {
 public:
  virtual ~IObjectStore() {}

  /*
   * Object access APIs.
   *
   * The given ObjectFetchContext must remain valid at least until the
   * resulting future is complete.
   */

  virtual folly::Future<std::shared_ptr<const Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<std::shared_ptr<const Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<std::shared_ptr<const Tree>> getTreeForCommit(
      const Hash& commitID,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<std::shared_ptr<const Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids,
      ObjectFetchContext& context) const = 0;
};
} // namespace eden
} // namespace facebook
