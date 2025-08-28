/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/synchronization/CallOnce.h>
#include <gtest/gtest_prod.h>
#include <stdlib.h>
#include <condition_variable>
#include <iosfwd>
#include <memory>
#include <optional>
#include <stdexcept>
#include <thread>
#include <unordered_map>
#include <variant>
#include <vector>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/telemetry/TraceBus.h"
#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/fuse/FuseDispatcher.h"
#include "eden/fs/inodes/FsChannel.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/utils/FsChannelTypes.h"
#include "eden/fs/utils/ProcessAccessLog.h"

#include <fmt/format.h>

#ifndef _WIN32
#include <sys/uio.h>
#endif

namespace folly {
struct Unit;
} // namespace folly

namespace facebook::eden {

class Notifier;
class FsEventLogger;
class FuseRequestContext;
class PrivHelper;
class StructuredLogger;

#ifndef _WIN32

using TraceDetailedArgumentsHandle = std::shared_ptr<void>;

struct FuseTraceEvent : TraceEventBase {
  enum Type : unsigned char {
    START,
    FINISH,
  };

  /**
   * Contains the useful fields of fuse_in_header to save TraceBus memory.
   */
  struct RequestHeader {
    explicit RequestHeader(const fuse_in_header& header)
        : nodeid{header.nodeid},
          opcode{header.opcode},
          uid{header.uid},
          gid{header.gid},
          pid{header.pid} {}

    uint64_t nodeid;
    uint32_t opcode;
    uint32_t uid;
    uint32_t gid;
    uint32_t pid;
  };

  FuseTraceEvent() = delete;

  static FuseTraceEvent start(uint64_t unique, const fuse_in_header& request) {
    return FuseTraceEvent{
        unique, request, StartDetails{std::unique_ptr<std::string>{}}};
  }

  static FuseTraceEvent
  start(uint64_t unique, const fuse_in_header& request, std::string arguments) {
    return FuseTraceEvent{
        unique,
        request,
        StartDetails{std::make_unique<std::string>(std::move(arguments))}};
  }

  static FuseTraceEvent finish(
      uint64_t unique,
      const fuse_in_header& request,
      std::optional<int64_t> result) {
    return FuseTraceEvent{unique, request, FinishDetails{result}};
  }

  Type getType() const {
    return std::holds_alternative<StartDetails>(details_) ? Type::START
                                                          : Type::FINISH;
  }

  uint64_t getUnique() const {
    return unique_;
  }

  const RequestHeader& getRequest() const {
    return request_;
  }

  const std::unique_ptr<std::string>& getArguments() const {
    return std::get<StartDetails>(details_).arguments;
  }

  const std::optional<int64_t>& getResponseCode() const {
    return std::get<FinishDetails>(details_).result;
  }

 private:
  struct StartDetails {
    /**
     * If detailed trace arguments have been requested, this field contains a
     * human-readable representation of the FUSE request arguments.
     *
     * It is heap-allocated to reduce memory usage in the common case that
     * detailed argument tracing is disabled.
     *
     * TODO: 32 bytes for an optional, immutable argument string is excessive.
     * fbstring would be better, but we could fit this in 16 or even 8 bytes.
     */
    std::unique_ptr<std::string> arguments;
  };

  struct FinishDetails {
    /**
     * If set, a response code was sent to the kernel.
     *
     * Negative values indicate errors, and non-negative success. Errors map to
     * the fuse_out_header::error value, negated.
     *
     * For requests where the kernel will maintain a reference to the returned
     * inode, result will contain fuse_entry_out::nodeid.
     */
    std::optional<int64_t> result;
  };

  using Details = std::variant<StartDetails, FinishDetails>;

  FuseTraceEvent(
      uint64_t unique,
      const fuse_in_header& request,
      Details&& details)
      : unique_{unique}, request_{request}, details_{std::move(details)} {}

  /**
   * FUSE generates its own unique ID per request, but it reuses them often, so
   * include our own permanently-unique IDs too.
   */
  uint64_t unique_;
  RequestHeader request_;
  Details details_;
};

class FuseChannel final : public FsChannel {
 public:
  enum class StopReason {
    RUNNING, // not stopped
    INIT_FAILED,
    UNMOUNTED,
    TAKEOVER,
    DESTRUCTOR,
    FUSE_READ_ERROR,
    FUSE_WRITE_ERROR,
    FUSE_TRUNCATED_REQUEST,
    WORKER_EXCEPTION,
  };

  struct StopData final : public FsStopData {
    bool isUnmounted() override {
      return !fuseDevice;
    }

    FsChannelInfo extractTakeoverInfo() override {
      return FuseChannelData{std::move(fuseDevice), fuseSettings};
    }

    /**
     * The reason why the FUSE channel was stopped.
     *
     * If multiple events occurred that triggered shutdown, only one will be
     * reported here.  (e.g., If a takeover was initiated and one of the worker
     * threads saw that the device was unmounted before it stopped.)
     */
    StopReason reason{StopReason::RUNNING};

    /**
     * The FUSE device for communicating with the kernel, if it is still valid
     * to use.
     *
     * This will be a closed File object if the FUSE device is no longer valid
     * (e.g., if it has been unmounted or if an error occurred on the FUSE
     * device).
     */
    folly::File fuseDevice;

    /**
     * The FUSE connection settings negotiated on the FUSE device.
     *
     * This will have valid data only when fuseDevice is also valid.
     * This can be passed to initializeFromTakeover() to start a new
     * FuseChannel object that uses this FuseDevice.
     */
    fuse_init_out fuseSettings = {};
  };

  struct OutstandingRequest {
    uint64_t unique;
    FuseTraceEvent::RequestHeader request;
    std::chrono::steady_clock::time_point requestStartTime;
  };

  /**
   * Construct the fuse channel and session structures that are
   * required by libfuse to communicate with the kernel using
   * a pre-existing fuseDevice descriptor.  The descriptor may
   * have been obtained via privilegedFuseMount() or may have
   * been passed to us as part of a graceful restart procedure.
   *
   * The caller is expected to follow up with a call to the
   * initialize() method to perform the handshake with the
   * kernel and set up the thread pool.
   *
   * privHelper -
   *      a helper object that can be used to perform privileged actions like
   *      mounting/unmounting the FUSE device.
   * fuseDevice -
   *      the file to use for communication with the kernel. Fuse requests and
   *      responses go through here.
   * mountPath -
   *      the absolute path to the mount point on disk.
   * threadPool -
   *      the thread pool to use for processing FUSE requests. Note this is not
   *      used to read fuse requests, but the processing that is not run on
   *      another thread pool in EdenFS is run here.
   * numThreads -
   *      the number of worker threads to read fuse requests off the fuseDevice.
   * dispatcher -
   *      once parsed requests are passed off to the dispatcher for handling,
   *      this is the connection to the rest of EdenFS.
   * straceLogger -
   *      a logger to use for logging strace/syscall like events.
   * processInfoCache -
   *      a cache of client process information (pid, command line, parent,
   * etc). fsEventLogger - legacy telemetry on filesystem access.
   * structuredLogger -
   *      This is a logger for error events. Inside a Meta environment, these
   *      events are exported off the machine this EdenFS instance is running
   * on. This is where you log anomalous things that you want to monitor across
   * the fleet. requestTimeout - internal timeout for how long the FuseChannel
   * will give the lower levels of EdenFS to process an event. ETIMEDOUT will be
   * returned to the kernel if a request exceeds this amount of time. notifier -
   *      used to flag abnormal EdenFS behavior to users.
   * caseSensitive -
   *      whether or not the mount is case sensitive.
   * requireUtf8Path -
   *      whether the mount requires utf-8 compliant paths.
   * maximumBackgroundRequests -
   *      The max number of background requests the kernel will send. The
   *      libfuse documentation says this only applies to background requests
   *      like readahead prefetches and direct I/O, but we have empirically
   *      observed that, on Linux, without setting this value, `rg -j 200`
   *      limits the number of active FUSE requests to 16.
   * maximumInFlightRequests -
   *      The max number of in-flight requests that EdenFS will process at once.
   *      New requests will block when this limit is reached and will wait until
   *      existing requests until existing requests complete. When set to zero,
   *      no rate limiting will be enforced.
   * highFuseRequestsLogInterval -
   *      How often to log when we see a high number of in-flight FUSE requests.
   *      This is used to prevent us from spamming data to scuba.
   * useWriteBackCache -
   *      Fuse may complete writes while they are cached in kernel before they
   *      are written to EdenFS.
   * fuseTraceBusCapacity -
   *      The maximum number of FuseTraceEvents that can be buffered in the
   *      trace bus at any one time. This data feeds into `eden trace fs`.
   */
  FuseChannel(
      PrivHelper* privHelper,
      folly::File fuseDevice,
      AbsolutePathPiece mountPath,
      std::shared_ptr<folly::Executor> threadPool,
      size_t numThreads,
      std::unique_ptr<FuseDispatcher> dispatcher,
      const folly::Logger* straceLogger,
      std::shared_ptr<ProcessInfoCache> processInfoCache,
      std::shared_ptr<FsEventLogger> fsEventLogger,
      const std::shared_ptr<StructuredLogger>& structuredLogger,
      folly::Duration requestTimeout,
      std::shared_ptr<Notifier> notifier,
      CaseSensitivity caseSensitive,
      bool requireUtf8Path,
      int32_t maximumBackgroundRequests,
      size_t maximumInFlightRequests,
      std::chrono::nanoseconds highFuseRequestsLogInterval,
      std::chrono::nanoseconds longRunningFSRequestThreshold,
      bool useWriteBackCache,
      size_t fuseTraceBusCapacity);

  FuseChannel(const FuseChannel&) = delete;
  FuseChannel(FuseChannel&&) = delete;
  FuseChannel& operator=(const FuseChannel&) = delete;
  FuseChannel& operator=(FuseChannel&&) = delete;

  /**
   * Destroy the FuseChannel.
   *
   * If the FUSE worker threads are still running, the destroy() will stop
   * them and wait for them to exit.
   *
   * destroy() must not be invoked from inside one of the worker threads.  For
   * instance, do not invoke the destructor from inside a Dispatcher callback.
   *
   * The FuseChannel object itself may not have been immediately deleted by the
   * time that destroy() returns.  destroy() will wait for all outstanding FUSE
   * requests to complete before it deletes the object.  However, it may return
   * before this happens if some FUSE requests are still pending and will
   * complete in a non-FUSE-worker thread.
   */
  void destroy() override;

  const char* getName() const override {
    return "fuse";
  }

  /**
   * Initialize the FuseChannel; until this completes successfully,
   * FUSE requests will not be serviced.
   *
   * This will first start one worker thread to wait for the INIT request from
   * the kernel and validate that we are compatible.  Once we have successfully
   * completed the INIT negotiation with the kernel we will start the remaining
   * FUSE worker threads and indicate success via the returned Future object.
   *
   * Returns a folly::Future that will become ready once the mount point has
   * been initialized and is ready for I/O.  This Future will complete inside
   * one of the FUSE worker threads.
   *
   * The initialization Future will return a new StopFuture object that will
   * be fulfilled when the FuseChannel has stopped.  This StopFuture can be
   * used to detect if the FuseChannel has been unmounted or stopped because of
   * an error or any other reason.  The StopFuture may be fulfilled inside one
   * of the FUSE worker threads that is in the process of shutting down.
   * Callers should normally use via() to perform any additional work in
   * another executor thread.
   */
  FOLLY_NODISCARD folly::Future<StopFuture> initialize() override;

  /**
   * Initialize the FuseChannel when taking over an existing FuseDevice.
   *
   * This is used when performing a graceful restart of Eden, where we are
   * taking over a FUSE connection that was already initialized by a previous
   * process.
   *
   * The connInfo parameter specifies the connection data that was already
   * negotiated by the previous owner of the FuseDevice.
   *
   * This function will immediately set up the thread pool used to service
   * incoming fuse requests.
   *
   * Returns a StopFuture that will be fulfilled when the FuseChannel has
   * stopped.  This future can be used to detect if the FuseChannel has been
   * unmounted or stopped because of an error or any other reason.
   */
  StopFuture initializeFromTakeover(fuse_init_out connInfo);

  /**
   * Uses the configured PrivHelper to unmount this FUSE mount from the
   * filesystem.
   *
   * That kicks off an ENODEV error from the FUSE device, and shuts down the
   * FuseChannel. The future returned by initialize() will be fulfilled with a
   * non-takeover StopData.
   */
  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> unmount(
      UnmountOptions options) override;

  /**
   * Request that the FuseChannel stop processing new requests, and prepare
   * to hand over the FuseDevice to another process.
   */
  bool takeoverStop() override {
    requestSessionExit(StopReason::TAKEOVER);
    return true;
  }

  /**
   * Request that the kernel invalidate its cached data for the specified
   * inode.
   *
   * This operation is performed asynchronously.  completeInvalidations() can be
   * called if you need to determine when this operation has completed.
   *
   * @param ino the inode number
   * @param off the offset in the inode where to start invalidating
   *            or negative to invalidate attributes only
   * @param len the amount of cache to invalidate or 0 for all
   */
  void invalidateInode(InodeNumber ino, int64_t off, int64_t len);

  /**
   * Request that the kernel invalidate its cached data for the specified
   * directory entry.
   *
   * This operation is performed asynchronously.  completeInvalidations() can be
   * called if you need to determine when this operation has completed.
   *
   * @param parent inode number
   * @param name file name
   */
  void invalidateEntry(InodeNumber parent, PathComponentPiece name);

  /*
   * Request that the kernel invalidate its cached data for the specified
   * inodes.
   *
   * This operation is performed asynchronously.  completeInvalidations() can be
   * called if you need to determine when this operation has completed.
   *
   * @param range a range of inodes
   */
  void invalidateInodes(folly::Range<InodeNumber*> range);

  /**
   * Wait for all currently scheduled invalidateInode() and invalidateEntry()
   * operations to complete.
   *
   * The returned Future will complete once all invalidation operations
   * scheduled before this completeInvalidations() call have finished.
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> completeInvalidations() override;

  /**
   * Sends a reply to a kernel request that consists only of the error
   * status (no additional payload).
   * `err` may be 0 (indicating success) or a positive errno value.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  void replyError(const fuse_in_header& request, int err);

  /**
   * Sends a raw data packet to the kernel.
   * The data may be scattered across a number of discrete buffers;
   * this method uses writev to send them to the kernel as a single unit.
   * The kernel, and thus this method, assumes that the start of this data
   * is a fuse_out_header instance.  This method will sum the iovec lengths
   * to compute the correct value to store into fuse_out_header::len.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  void sendRawReply(const iovec iov[], size_t count) const;

  /**
   * Sends a range of contiguous bytes as a reply to the kernel.
   * request holds the context of the request to which we are replying.
   * `bytes` is the payload to send in addition to the successful status
   * header generated by this method.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  void sendReply(const fuse_in_header& request, folly::ByteRange bytes) const;

  void sendReply(const fuse_in_header& request, folly::StringPiece bytes)
      const {
    sendReply(request, folly::ByteRange{bytes});
  }

  /**
   * Sends a reply to a kernel request, consisting of multiple parts.
   * The `vec` parameter holds an array of payload components and is moved
   * in to this method which then prepends a fuse_out_header and passes
   * control along to sendRawReply().
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  void sendReply(const fuse_in_header& request, folly::fbvector<iovec>&& vec)
      const;

  /**
   * Sends a reply to a kernel request potentially consisting of multiple
   * segments.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  void sendReply(const fuse_in_header& request, const folly::IOBuf& buf) const;

  /**
   * Sends a reply to the kernel.
   * The payload parameter is typically a fuse_out_XXX struct as defined
   * in the appropriate fuse_kernel_XXX.h header file.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  template <typename T>
  void sendReply(const fuse_in_header& request, const T& payload) const {
    static_assert(std::is_standard_layout_v<T>);
    static_assert(std::is_trivial_v<T>);
    sendReply(
        request,
        folly::ByteRange{
            reinterpret_cast<const uint8_t*>(&payload), sizeof(T)});
  }

  /**
   * Returns the approximate number of outstanding FUSE requests. Since
   * telemetry is tracked on a background thread, this number may very slightly
   * lag reality.
   *
   * As another option, Linux kernel maintains a count, accessible via
   * /sys/fs/fuse/connections/${conn_id}/waiting
   */
  std::vector<FuseChannel::OutstandingRequest> getOutstandingRequests();

  /**
   * While the returned handle is alive, FuseTraceEvents published on the
   * TraceBus will have detailed argument strings.
   */
  TraceDetailedArgumentsHandle traceDetailedArguments() const;

  TraceBus<FuseTraceEvent>& getTraceBus() {
    return *traceBus_;
  }

  ProcessAccessLog& getProcessAccessLog() override {
    return processAccessLog_;
  }

  std::shared_ptr<StructuredLogger> getStructuredLogger() const {
    return structuredLogger_;
  }

  std::chrono::nanoseconds getLongRunningFSRequestThreshold() const {
    return longRunningFSRequestThreshold_;
  }

  ImmediateFuture<folly::Unit> waitForPendingWrites() override {
    return folly::unit;
  }

  std::shared_ptr<Notifier> getNotifier() const {
    return notifier_;
  }

  size_t getRequestMetric(RequestMetricsScope::RequestMetric metric) const;

 private:
  /**
   * All of our mutable state that may be accessed from the worker threads,
   * and therefore requires synchronization.
   */
  struct State {
    std::vector<std::thread> workerThreads;

    /**
     * We count live requests to avoid shutting down the session while responses
     * are pending.
     */
    size_t pendingRequests{0};

    /**
     * We log to scuba when clients see a high number of FUSE requests. It's
     * likely we exceed this threshold many times in a row, so we only log
     * once every EdenConfig::highFsRequestsLogInterval. This keeps track of
     * the last time we logged to scuba.
     */
    std::chrono::steady_clock::time_point lastHighFuseRequestsLog_;

    /**
     * We track the number of stopped threads, to know when we are done and can
     * signal sessionCompletePromise_.  We only want to signal
     * sessionCompletePromise_ after initialization is successful and then all
     * threads have stopped.
     *
     * If an error occurs during initialization we may have started some but
     * not all of the worker threads.  We do not want to signal
     * sessionCompletePromise_ in this case--we will return the error from
     * initialize() or takeoverInitialize() instead.
     */
    size_t stoppedThreads{0};

    /**
     * If destroyPending is true, the FuseChannel object should be
     * automatically destroyed when the last outstanding request finishes.
     */
    bool destroyPending{false};

    /**
     * If the FuseChannel is stopped or stopping, the reason why it is
     * stopping.  This is set to RUNNING while the FuseDevice is initializing
     * or running.
     */
    StopReason stopReason{StopReason::RUNNING};
  };

  /**
   * Only written by the TraceBus thread, but must be synchronized for readers.
   */
  struct TelemetryState {
    std::unordered_map<uint64_t, OutstandingRequest> requests;
  };

  struct DataRange {
    DataRange(int64_t offset, int64_t length);

    int64_t offset;
    int64_t length;
  };
  enum class InvalidationType : uint32_t {
    INODE,
    DIR_ENTRY,
    FLUSH,
  };
  struct InvalidationEntry {
    InvalidationEntry(InodeNumber inode, int64_t offset, int64_t length);
    InvalidationEntry(InodeNumber inode, PathComponentPiece name);
    explicit InvalidationEntry(folly::Promise<folly::Unit> promise);
    InvalidationEntry(InvalidationEntry&& other) noexcept(
        std::is_nothrow_move_constructible_v<PathComponent> &&
        std::is_nothrow_move_constructible_v<folly::Promise<folly::Unit>> &&
        std::is_nothrow_move_constructible_v<DataRange>);
    ~InvalidationEntry();

    InvalidationType type;
    InodeNumber inode;
    union {
      PathComponent name;
      DataRange range;
      folly::Promise<folly::Unit> promise;
    };
  };
  struct InvalidationQueue {
    std::vector<InvalidationEntry> queue;
    bool stop{false};
  };

  friend struct fmt::formatter<facebook::eden::FuseChannel::InvalidationEntry>;

  FRIEND_TEST(FuseChannelTest, formatting_inode);
  FRIEND_TEST(FuseChannelTest, formatting_dir);
  FRIEND_TEST(FuseChannelTest, formatting_flush);
  FRIEND_TEST(FuseChannelTest, formatting_unknown);
  /**
   * Private destructor.
   *
   * FuseChannel objects must always be allocated on the heap, and destroyed by
   * calling FuseChannel::destroy().  Users are not allowed to destroy
   * FuseChannel objects directly.
   *
   * FuseChannel::destroy() will delete the FuseChannel once all outstanding
   * requests have completed.
   */
  ~FuseChannel();

 public:
  ImmediateFuture<folly::Unit> fuseRead(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseWrite(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseLookup(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseForget(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseGetAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseSetAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseReadLink(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseSymlink(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseMknod(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseMkdir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseUnlink(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseRmdir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseRename(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseLink(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseOpen(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseStatFs(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseRelease(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseFsync(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseSetXAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseGetXAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseListXAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseRemoveXAttr(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseFlush(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseOpenDir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseReadDir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseReleaseDir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseFsyncDir(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseAccess(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseCreate(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseBmap(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseBatchForget(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);
  ImmediateFuture<folly::Unit> fuseFallocate(
      FuseRequestContext& request,
      const fuse_in_header& header,
      folly::ByteRange arg);

 private:
  void setThreadSigmask();
  void initWorkerThread() noexcept;
  void fuseWorkerThread() noexcept;
  void invalidationThread() noexcept;
  void stopInvalidationThread();
  void sendInvalidation(InvalidationEntry& entry);
  void sendInvalidateInode(InodeNumber ino, int64_t off, int64_t len);
  void sendInvalidateEntry(InodeNumber parent, PathComponentPiece name);
  void readInitPacket();
  void startWorkerThreads();

  /**
   * sessionComplete() will fulfill the sessionCompletePromise_.
   *
   * Beware: calling sessionComplete() should be the very last statement in any
   * method that calls it.  It may destroy the FuseChannel object before it
   * returns.
   */
  void sessionComplete(folly::Synchronized<State>::LockedPtr state);

  static bool isFuseDeviceValid(StopReason reason) {
    // The FuseDevice may still be used if the FuseChannel was stopped due to a
    // takeover request or because the FuseChannel object was destroyed without
    // ever being unmounted.  It is also still valid if the FuseChannel is
    // still running.
    //
    // In all other cases the FuseDevice should no longer be used.
    return (
        reason == StopReason::RUNNING || reason == StopReason::TAKEOVER ||
        reason == StopReason::DESTRUCTOR);
  }

  /**
   * Dispatches fuse requests until the session is torn down.
   * This function blocks until the fuse session is stopped.
   * The intent is that this is called from each of the
   * fuse worker threads provided by the MountPoint.
   */
  void processSession();

  /**
   * Requests that the worker threads terminate their processing loop.
   */
  void requestSessionExit(StopReason reason);
  void requestSessionExit(
      const folly::Synchronized<State>::LockedPtr& state,
      StopReason reason);

  PrivHelper* const privHelper_;

  /*
   * Constant state that does not change for the lifetime of the FuseChannel
   */
  const size_t bufferSize_{0};
  std::shared_ptr<folly::Executor> threadPool_;
  const size_t numThreads_;
  std::unique_ptr<FuseDispatcher> dispatcher_;
  const folly::Logger* const straceLogger_;
  const std::shared_ptr<StructuredLogger> structuredLogger_;
  const AbsolutePath mountPath_;
  const folly::Duration requestTimeout_;
  std::shared_ptr<Notifier> const notifier_;
  CaseSensitivity caseSensitive_;
  bool requireUtf8Path_;
  /**
   * The maximum number of concurrent background FUSE requests we allow the
   * kernel to send us. background should mean things like readahead prefetches
   * and direct I/O, but may include things that seem like more traditionally
   * foreground I/O. What counts as "background" seems to be up to the
   * discretion of the kernel. This is managed by the kernel by setting
   * max_background in fuse_init_out
   */
  int32_t maximumBackgroundRequests_;
  /**
   * The maximum number of requests that can be processed at one time. This is
   * only enforced when the value is > 0.
   */
  size_t maximumInFlightRequests_;
  /**
   * We log when the number of pending requests exceeds maximumInFlightRequests,
   * however to avoid spamming the logs once per highFuseRequestsLogInterval.
   */
  std::chrono::nanoseconds highFuseRequestsLogInterval_;
  /**
   * The duration that must elapse before we consider a FUSE request to be
   * "long running" and therefore log it with StructuredLogger. This value
   * is configured with EdenConfig::longRunningFSRequestThreshold.
   */
  std::chrono::nanoseconds longRunningFSRequestThreshold_;
  bool useWriteBackCache_;

  /*
   * connInfo_ is modified during the initialization process,
   * but constant once initialization is complete.
   */
  std::optional<fuse_init_out> connInfo_;

  /*
   * fuseDevice_ is constant while the worker threads are running.
   *
   * This is constant for most of the lifetime of the FuseChannel object.
   * It can be modified during shutdown so that it can be returned as part of
   * the StopData.  However, there is guaranteed to be external synchronization
   * around this event:
   * - If the stop can occur as the last FUSE worker thread shuts down.
   *   No other FUSE worker threads can access fuseDevice_ after this point,
   *   and the FuseChannel destructor will join the threads before destroying
   *   fuseDevice_.
   * - If the stop can occur as when the last outstanding FUSE request
   *   completes, after all FUSE worker threads have stopped.  In this case no
   *   other threads are accessing the FuseChannel and it will be immediately
   *   destroyed in the same thread that creates the StopData object.
   */
  folly::File fuseDevice_;

  /*
   * Mutable state that is accessed from the worker threads.
   * All of this state uses locking or other synchronization.
   */
  std::atomic<bool> stop_{false};
  folly::once_flag unmountLogFlag_;
  folly::Synchronized<State> state_;
  folly::Promise<StopFuture> initPromise_;
  folly::Promise<FsStopDataPtr> sessionCompletePromise_;

  folly::Synchronized<TelemetryState> telemetryState_;

  // To prevent logging unsupported opcodes twice.
  folly::Synchronized<std::unordered_set<FuseOpcode>> unhandledOpcodes_;

  // State for sending inode invalidation requests to the kernel
  // These are processed in their own dedicated thread.
  folly::Synchronized<InvalidationQueue, std::mutex> invalidationQueue_;
  std::condition_variable invalidationCV_;
  std::thread invalidationThread_;

  ProcessAccessLog processAccessLog_;

  // this tracks metrics for live FUSE requests, this is a thread local
  // to avoid contention between the FuseWorkerThreads as they kick off
  // requests.
  // each thread local is a shared pointer to keep the tracker from being
  // destroyed when the owning FuseWorkerThread ends if there are outstanding
  // requests as these may outlive the spawning worker thread.
  class ThreadLocalTag {};
  folly::ThreadLocal<
      std::shared_ptr<RequestMetricsScope::LockedRequestWatchList>,
      ThreadLocalTag>
      liveRequestWatches_;

  std::vector<TraceSubscriptionHandle<FuseTraceEvent>>
      traceSubscriptionHandles_;

  /*
   * TraceBus subscribers can indicate they would like detailed argument strings
   * for FUSE requests. These are relatively expensive to compute, so argument
   * strings in FuseTraceEvent are empty when zero.
   *
   * It's a bit ridiculous to heap-allocate an int, but subscribers may outlive
   * the FuseChannel.
   */
  std::shared_ptr<std::atomic<size_t>> traceDetailedArguments_;

  // This should be the last field, as subscriber functions close over [this],
  // and it's not until TraceBus is destroyed that it's guaranteed that
  // subscriber functions will no longer run.
  // This shared_ptr will never be copied.
  std::shared_ptr<TraceBus<FuseTraceEvent>> traceBus_;
};

folly::StringPiece fuseOpcodeName(uint32_t opcode);
ProcessAccessLog::AccessType fuseOpcodeAccessType(uint32_t opcode);

template <typename... Args>
std::unique_ptr<FuseChannel, FsChannelDeleter> makeFuseChannel(Args&&... args) {
  return std::unique_ptr<FuseChannel, FsChannelDeleter>{
      new FuseChannel(std::forward<Args>(args)...)};
}

class FuseDeviceUnmountedDuringInitialization : public std::runtime_error {
 public:
  explicit FuseDeviceUnmountedDuringInitialization(AbsolutePathPiece mountPath);
};

#endif
} // namespace facebook::eden

#ifndef _WIN32
namespace fmt {
template <>
struct formatter<facebook::eden::FuseChannel::InvalidationEntry>
    : formatter<string_view> {
  template <typename FormatContext>
  auto format(
      const facebook::eden::FuseChannel::InvalidationEntry& entry,
      FormatContext& ctx) {
    auto out = ctx.out();
    switch (entry.type) {
      case facebook::eden::FuseChannel::InvalidationType::INODE:
        return fmt::format_to(
            out,
            "(inode {}, offset {}, length {})",
            entry.inode,
            entry.range.offset,
            entry.range.length);
      case facebook::eden::FuseChannel::InvalidationType::DIR_ENTRY:
        return fmt::format_to(
            out, "(inode {}, child \"{}\")", entry.inode, entry.name);
      case facebook::eden::FuseChannel::InvalidationType::FLUSH:
        return fmt::format_to(out, "(invalidation flush)");
      default:
        return fmt::format_to(
            out,
            "(unknown invalidation type {} inode {})",
            static_cast<int>(entry.type),
            entry.inode);
    }
  }
};
} // namespace fmt
#endif
