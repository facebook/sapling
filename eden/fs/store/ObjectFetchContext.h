/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <string_view>
#include <unordered_map>

#include <folly/portability/SysTypes.h>

#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/utils/RefPtr.h"

namespace facebook::eden {

class ObjectId;

class ObjectFetchContext;

using ObjectFetchContextPtr = RefPtr<ObjectFetchContext>;

/**
 * ObjectStore calls methods on this context when fetching objects.
 * It's primarily used to track when and why source control objects are fetched.
 */
class ObjectFetchContext : public RefCounted {
 public:
  /**
   * Which object type was fetched.
   *
   * Suitable for use as an index into an array of size kObjectTypeEnumMax
   */
  enum ObjectType : uint8_t {
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
  enum Origin : uint8_t {
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
   * Why did EdenFS fetch these objects?
   */
  enum Cause : uint8_t { Unknown, Fs, Thrift, Prefetch };

  ObjectFetchContext() = default;

  virtual ~ObjectFetchContext() = default;

  virtual void didFetch(ObjectType, const ObjectId&, Origin) {}

  virtual std::optional<pid_t> getClientPid() const {
    return std::nullopt;
  }

  /**
   * If known, returns the reason these objects were fetched.
   */
  virtual Cause getCause() const = 0;

  virtual std::optional<std::string_view> getCauseDetail() const {
    return std::nullopt;
  }

  virtual ImportPriority getPriority() const {
    return kDefaultImportPriority;
  }

  virtual const std::unordered_map<std::string, std::string>* getRequestInfo()
      const = 0;

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
  static ObjectFetchContextPtr getNullContext();

  /**
   * Return a no-op fetch context which has causeDetail field. This field will
   * be logged which in turn can point out "blind spots" in logging i.e. places
   * where null context should be replaces with a real context.
   *
   * Note that this function allocates and return a pointer to a newly allocated
   * memory. This pointer is intented to be used as static variable i.e. static
   * auto ptr = ObjectFetchContext::getNullContextWithCauseDetail("someval");
   */
  static ObjectFetchContextPtr getNullContextWithCauseDetail(
      std::string_view causeDetail);

 private:
  ObjectFetchContext(const ObjectFetchContext&) = delete;
  ObjectFetchContext& operator=(const ObjectFetchContext&) = delete;
};

} // namespace facebook::eden
