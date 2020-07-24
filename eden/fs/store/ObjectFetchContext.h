/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <optional>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ImportPriority.h"

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

  ObjectFetchContext() {}

  virtual ~ObjectFetchContext() = default;

  virtual void didFetch(ObjectType, const Hash&, Origin) {}

  virtual std::optional<pid_t> getClientPid() const {
    return std::nullopt;
  }

  virtual Cause getCause() const {
    return ObjectFetchContext::Cause::Unknown;
  }

  virtual ImportPriority getPriority() const {
    return ImportPriority::kNormal();
  }

  /**
   * Support deprioritizing in sub-classes.
   * Note: Normally, each ObjectFetchContext is designed to be used for only one
   * import (with NullObjectFetchContext being the only exception currenly).
   * Therefore, this method should only be called once on each
   * ObjectFetchContext object (when it is related to a process doing too much
   * fetches). However, implementations of this method should write the priority
   * change to log as debug information and watch out for unexpected uses of
   * ObjectFetchContext that cause it to be used for more than one import.
   */
  virtual void deprioritize(uint64_t) {}

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
