/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/fs/model/Hash.h"

namespace facebook {
namespace eden {

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
  enum ObjectType : unsigned {
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
  enum Origin : unsigned {
    FromMemoryCache,
    FromDiskCache,
    FromBackingStore,
    kOriginEnumMax,
  };

  /**
   * Which interface caused this object fetch
   */
  enum Cause : unsigned { Unknown, Fuse, Thrift };

  ObjectFetchContext() = default;
  virtual ~ObjectFetchContext() = default;
  virtual void didFetch(ObjectType, const Hash&, Origin) {}

  virtual std::optional<pid_t> getClientPid() const = 0;

  virtual Cause getCause() const = 0;

  /**
   * Return a no-op fetch context suitable when no tracking is desired.
   */
  static ObjectFetchContext& getNullContext();

 private:
  ObjectFetchContext(const ObjectFetchContext&) = delete;
  ObjectFetchContext& operator=(const ObjectFetchContext&) = delete;
};

} // namespace eden
} // namespace facebook
