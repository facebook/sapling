/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <optional>

#include <folly/Range.h>

#include "eden/fs/store/ImportPriority.h"

namespace facebook::eden {

class ObjectId;
class Hash20;

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
    /** The request didn't succeed */
    NotFetched,
    /** The request was serviced from a memory cache */
    FromMemoryCache,
    /** The request was serviced from a disk cache */
    FromDiskCache,
    /** The request was serviced with a network request */
    FromNetworkFetch,
    kOriginEnumMax,
  };

  /**
   * Which interface caused this object fetch
   */
  enum Cause : unsigned { Unknown, Fs, Thrift, Prefetch };

  ObjectFetchContext() {}

  virtual ~ObjectFetchContext() = default;

  virtual void didFetch(ObjectType, const ObjectId&, Origin) {}

  virtual std::optional<pid_t> getClientPid() const {
    return std::nullopt;
  }

  virtual Cause getCause() const {
    return ObjectFetchContext::Cause::Unknown;
  }

  virtual std::optional<folly::StringPiece> getCauseDetail() const {
    return std::nullopt;
  }

  virtual ImportPriority getPriority() const {
    return ImportPriority::kNormal();
  }

  virtual bool prefetchMetadata() const {
    return true;
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

  /**
   * Return a no-op fetch context which has causeDetail field. This field will
   * be logged which in turn can point out "blind spots" in logging i.e. places
   * where null context should be replaces with a real context.
   * Note that this function allocates and return a pointer to a newly allocated
   * memory. This pointer is intented to be used as static variable i.e. static
   * auto ptr = ObjectFetchContext::getNullContextWithCauseDetail("someval");
   */
  static ObjectFetchContext* getNullContextWithCauseDetail(
      folly::StringPiece causeDetail);

 private:
  ObjectFetchContext(const ObjectFetchContext&) = delete;
  ObjectFetchContext& operator=(const ObjectFetchContext&) = delete;
};

} // namespace facebook::eden
