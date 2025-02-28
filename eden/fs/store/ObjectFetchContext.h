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

#include "eden/common/os/ProcessId.h"
#include "eden/common/utils/RefPtr.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/telemetry/EdenStats.h"

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
    BlobAuxData,
    Tree,
    TreeAuxData,
    RootTree,
    ManifestForRoot,
    PrefetchBlob,
    kObjectTypeEnumMax,
  };

  /**
   * The source of the data that was fetched
   */
  enum class FetchedSource : uint8_t {
    /** The data was fetched from a local source */
    Local,
    /** The data was fetched from a remote source */
    Remote,
    /**
     * The data will be fetched from local or remote source.
     * We don't know the source yet.
     */
    Unknown,
  };

  /**
   * It shows that the fetch was successful or not.
   */
  enum class FetchResult : uint8_t {
    /** The fetch was successful */
    Success,
    /** The fetch failed */
    Failure,
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
   * The enum values are based on the priority of the request
   * that can trigger different behavior in the backingstore.
   * If you add a new cause, ensure the priority order is correct.
   */
  enum Cause : uint8_t {
    /** Lowest Priority - unknown origination */
    Unknown = 0,
    /** The request originated from a Thrift prefetch endpoint */
    Prefetch = 1,
    /** The request originated from a Thrift endpoint */
    Thrift = 2,
    /** Highest Priority - The request originated from FUSE/NFS/PrjFS */
    Fs = 3,
  };

  ObjectFetchContext() = default;

  virtual ~ObjectFetchContext() = default;

  virtual void didFetch(ObjectType, const ObjectId&, Origin) {}

  virtual OptionalProcessId getClientPid() const {
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

  void setFetchedSource(
      FetchedSource fetchedSource,
      ObjectType type,
      EdenStatsPtr stats) {
    // There is no stat increment for FetchedSource::Unknown
    if (saplingStatsMap_.find({fetchedSource, type}) !=
        saplingStatsMap_.end()) {
      stats->increment(saplingStatsMap_[{fetchedSource, type}]);
    }
    fetchedSource_ = fetchedSource;
  }

  FetchedSource getFetchedSource() const {
    return fetchedSource_;
  }

  // RequestInfo keys used by ReCasBackingStore
  inline static const std::string kSessionIdField = "session-id";
  inline static const std::string kCacheSessionIdField = "cache-session-id";
  // RequestInfo keys used by SaplingNativeBackingStore
  inline static const std::string kClientCorrelator = "client-correlator";
  inline static const std::string kClientEntryPoint = "client-entrypoint";

  virtual const std::unordered_map<std::string, std::string>* getRequestInfo()
      const = 0;

  /**
   * Support deprioritizing in sub-classes.
   * Note: Normally, each ObjectFetchContext is designed to be used for only one
   * import (with NullObjectFetchContext being the only exception currently).
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

  /**
   * Return a no-op fetch context with Cause::Fs for tests.
   */
  static ObjectFetchContextPtr getNullFsContext();

  /**
   * Return a no-op fetch context with Cause::Prefetch for tests.
   */
  static ObjectFetchContextPtr getNullPrefetchContext();

 private:
  ObjectFetchContext(const ObjectFetchContext&) = delete;
  ObjectFetchContext& operator=(const ObjectFetchContext&) = delete;

  FetchedSource fetchedSource_{FetchedSource::Unknown};

  std::unordered_map<
      std::tuple<FetchedSource, ObjectType>,
      StatsGroupBase::Counter SaplingBackingStoreStats::*>
      saplingStatsMap_ = {
          {{FetchedSource::Local, ObjectType::Tree},
           &SaplingBackingStoreStats::fetchTreeLocal},
          {{FetchedSource::Local, ObjectType::TreeAuxData},
           &SaplingBackingStoreStats::fetchTreeAuxDataLocal},
          {{FetchedSource::Local, ObjectType::RootTree},
           &SaplingBackingStoreStats::getRootTreeLocal},
          {{FetchedSource::Local, ObjectType::ManifestForRoot},
           &SaplingBackingStoreStats::importManifestForRootLocal},
          {{FetchedSource::Local, ObjectType::Blob},
           &SaplingBackingStoreStats::fetchBlobLocal},
          {{FetchedSource::Local, ObjectType::BlobAuxData},
           &SaplingBackingStoreStats::fetchBlobAuxDataLocal},
          {{FetchedSource::Local, ObjectType::PrefetchBlob},
           &SaplingBackingStoreStats::prefetchBlobLocal},
      };
};

// For fbcode/eden/scm/lib/backingstore/src/ffi.rs
using FetchCause = ObjectFetchContext::Cause;

} // namespace facebook::eden
