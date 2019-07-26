/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#include <stdlib.h>
#include <sys/uio.h>
#include <condition_variable>
#include <iosfwd>
#include <memory>
#include <optional>
#include <stdexcept>
#include <thread>
#include <unordered_map>
#include <vector>

#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/fuse/InodeNumber.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcessAccessLog.h"

namespace folly {
class RequestContext;
struct Unit;
} // namespace folly

namespace facebook {
namespace eden {

class Dispatcher;

class FuseChannel {
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
  struct StopData {
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
  using StopFuture = folly::SemiFuture<StopData>;

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
   */
  FuseChannel(
      folly::File&& fuseDevice,
      AbsolutePathPiece mountPath,
      size_t numThreads,
      Dispatcher* const dispatcher,
      std::shared_ptr<ProcessNameCache> processNameCache);

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
  void destroy();

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
  FOLLY_NODISCARD folly::Future<StopFuture> initialize();

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

  // Forbidden copy constructor and assignment operator
  FuseChannel(FuseChannel const&) = delete;
  FuseChannel& operator=(FuseChannel const&) = delete;

  /**
   * Request that the FuseChannel stop processing new requests, and prepare
   * to hand over the FuseDevice to another process.
   */
  void takeoverStop() {
    requestSessionExit(StopReason::TAKEOVER);
  }

  /**
   * Request that the kernel invalidate its cached data for the specified
   * inode.
   *
   * This operation is performed asynchronously.  flushInvalidations() can be
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
   * This operation is performed asynchronously.  flushInvalidations() can be
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
   * This operation is performed asynchronously.  flushInvalidations() can be
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
   * scheduled before this flushInvalidations() call have finished.  This
   * future will normally be completed in the FuseChannel's invalidation
   * thread.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> flushInvalidations();

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
   * Sends a reply to the kernel.
   * The payload parameter is typically a fuse_out_XXX struct as defined
   * in the appropriate fuse_kernel_XXX.h header file.
   *
   * throws system_error if the write fails.  Writes can fail if the
   * data we send to the kernel is invalid.
   */
  template <typename T>
  void sendReply(const fuse_in_header& request, const T& payload) const {
    sendReply(
        request,
        folly::ByteRange{reinterpret_cast<const uint8_t*>(&payload),
                         sizeof(T)});
  }

  /**
   * Function to get outstanding fuse requests.
   */
  std::vector<fuse_in_header> getOutstandingRequests();

  ProcessAccessLog& getProcessAccessLog() {
    return processAccessLog_;
  }

 private:
  struct HandlerEntry;
  using HandlerMap = std::unordered_map<uint32_t, HandlerEntry>;

  /**
   * All of our mutable state that may be accessed from the worker threads,
   * and therefore requires synchronization.
   */
  struct State {
    uint64_t nextRequestId{1};
    std::unordered_map<uint64_t, std::weak_ptr<folly::RequestContext>> requests;
    std::vector<std::thread> workerThreads;

    /*
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
    InvalidationEntry(InvalidationEntry&& other) noexcept;
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
  friend std::ostream& operator<<(
      std::ostream& os,
      const InvalidationEntry& entry);

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

  bool isReadOperation(FuseOpcode opcode);
  bool isWriteOperation(FuseOpcode opcode);

  folly::Future<folly::Unit> fuseRead(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseWrite(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseLookup(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseForget(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseGetAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseSetAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseReadLink(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseSymlink(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseMknod(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseMkdir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseUnlink(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseRmdir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseRename(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseLink(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseOpen(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseStatFs(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseRelease(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseFsync(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseSetXAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseGetXAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseListXAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseRemoveXAttr(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseFlush(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseOpenDir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseReadDir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseReleaseDir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseFsyncDir(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseAccess(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseCreate(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseBmap(
      const fuse_in_header* header,
      const uint8_t* arg);
  folly::Future<folly::Unit> fuseBatchForget(
      const fuse_in_header* header,
      const uint8_t* arg);

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

  /*
   * Constant state that does not change for the lifetime of the FuseChannel
   */
  const size_t bufferSize_{0};
  const size_t numThreads_;
  Dispatcher* const dispatcher_{nullptr};
  const AbsolutePath mountPath_;

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
   *   and the FuseChannel destructor will join the threads before destryoing
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
  folly::Promise<StopData> sessionCompletePromise_;

  // To prevent logging unsupported opcodes twice.
  folly::Synchronized<std::unordered_set<FuseOpcode>> unhandledOpcodes_;

  // State for sending inode invalidation requests to the kernel
  // These are processed in their own dedicated thread.
  folly::Synchronized<InvalidationQueue, std::mutex> invalidationQueue_;
  std::condition_variable invalidationCV_;
  std::thread invalidationThread_;

  ProcessAccessLog processAccessLog_;

  static const HandlerMap handlerMap_;
};

/**
 * FuseChannelDeleter acts as a deleter argument for std::shared_ptr or
 * std::unique_ptr.
 */
class FuseChannelDeleter {
 public:
  void operator()(FuseChannel* channel) {
    channel->destroy();
  }
};

class FuseDeviceUnmountedDuringInitialization : public std::runtime_error {
 public:
  explicit FuseDeviceUnmountedDuringInitialization(AbsolutePathPiece mountPath);
};

} // namespace eden
} // namespace facebook
