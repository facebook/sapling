# Eden's Threading Strategy

## FsChannelThreadPool

This thread pool is used by a variety of FsChannel implementations. The main
uses will be described below. There are `fschannel:num-servicing-threads`
(defaults to `ncores` number of threads) for handling any request put on the
FsChannel thread pool. This thread pool is unbounded due to deadlocks caused by
FsChannelThreads being blocked trying to add to a full queue which can only be
emptied by FsChannelThreads when using a bounded thread pool. As of February
2025, there is no limit on the number of inflight requests that can be active at
once, but there is work in progress to add a rate limiter to this layer.

## FUSE

### Dispatch

There are `fuse:NumDispatcherThreads` (defaults to 16 as of Mar 2024) that block
on reading from the FUSE device. `FuseChannel::fuseWorkerThread()` defines the
work that these threads do. The reason we do blocking reads is to avoid two
syscalls on an incoming event: an epoll wakeup plus a read. It's possible that
these threads will completely handle a request from start to finish, but work
can also be handed off to the `FuseChannel::threadPool_` if the result is not
immediately ready.

Note: there is a FUSE socket per mount. So if you have 3 mounts, there will be
`3*fuse:NumDispatcherThreads` threads.

### Fuse Channel's use of `ServerState::fsChannelThreadPool_`

All FuseChannels share a `FuseChannel::threadPool_` (aka
`ServerState::fsChannelThreadPool_`). The FUSE threads generally do any
filesystem work directly rather than putting work on another thread. However, if
work is offloaded to another thread, then it is often moved to the
`FuseChannel::threadPool_`.

## NFS

Each NFS mount is assigned a dedicated `NFSServer` (this is somewhat atypical,
as usually there would be 1 NFS server running that processes requests for all
mounts). EdenFS also creates a single `mountd` instance that's used to register
all mounts. These use a mix of EventBases and thread pools to execute work.

### Registration (`mountd`)

All `mountd` processing happens on the EventBase that's passed into the `mountd`
constructor.

### Dispatch (`NFSServer`)

NFS requests are received by a dedicated EventBase that reads from the socket
that is assigned to each NFS Server (see
`RpcConnectionHandler::tryConsumeReadBuffer()`). The only work done on the
EventBase is reading requests from the socket. All other work is placed onto the
thread pool that was passed into the RPCConnectionHandler constructor. As of Nov
2024, the thread pool that's used to process these requests is the
`NfsServer::threadPool_` (aka `ServerState::fsChannelThreadPool_`).

## ProjFS

### Notifications

ProjectedFS notifications are sent to EdenFS asynchronously. EdenFS then
processes these notifications in the order that it receives them. The
notifications are processed on a single threaded, unbounded, sequenced executor
(`PrjfsDispatcher::notificationExecutor_`).

### Callbacks (unfinished)

If the results of notification callbacks are not immediately ready/available,
these callbacks are then detached from the `notificationExecutor_` and ran on
the global CPU executor (via `detachAndCompleteCallback()`).

### Invalidation

Eden runs invalidation on a separate thread pool
(`PrjfsChannel::invalidationThreadPool_`) to protect Eden against PrjFS
re-entrancy. i.e. If PrjFS makes a callback to Eden during the invalidation, we
don't want to be blocking the same thread pool that needs to handle that
callback. PrjFS is probably not re-entrant in that way, but better safe than
sorry. The number of invalidation threads is configured using
`prjfs:num-invalidation-threads` (defaults to 1 as of Nov 2024).

## Thrift

The Thrift server uses IO threads (defaults to `ncores`, configurable via
EdenConfig's `thrift:num-workers`). These threads are held in the
`ThriftServer::ioThreadPool_`. We don't change the default number (`ncores`) of
Thrift CPU threads. The IO threads receive incoming requests, but
serialization/deserialization and actually handling the request is done on the
CPU threads.

## Sapling Requests

Note that each SaplingBackingStore has its own pools, and there is one
SaplingBackingStore per underlying Sapling repository.

### Initial Processing

The `SaplingBackingStore::threads_` pool contains 32 (as of Nov 2024,
configurable via EdenConfig's `backingstore:num-servicing-threads`) threads on
which the SaplingBackingStore farms work out to the (blocking)
SaplingNativeBackingStore. Sapling has its own thread pools and SaplingAPI
batching logic, most of which is opaque to EdenFS. However, we can control the
maximum size of batches for requests that EdenFS sends to Sapling. NOTE: these
do not affect how Sapling batches requests it sends to the server via
SaplingAPI, but it may influence the batch sizes because it controls the volume
of requests that are sent to Sapling at once.

- `hg:import-batch-size`: controls how many blob requests we send to
  SaplingNativeBackingStore at once
- `hg:import-batch-size-tree`: controls how many tree requests we send to
  SaplingNativeBackingStore at once
- `hg:import-batch-size-blobmeta`: controls how many blob aux data requests we
  send to SaplingNativeBackingStore at once
- `hg:import-batch-size-blobmeta`: controls how many tree aux data requests we
  send to SaplingNativeBackingStore at once

### Post Processing

Because importing from Sapling is high-latency and mostly blocking, we avoid
doing any post-import computation on the `SaplingBackingStore::threads_` pool,
so these requests are placed onto the `SaplingBackingStore::serverThreadPool_`
(aka `ServerState::threadPool_` aka `EdenCPUThreadPool`).

## Miscellaneous tasks

Eden also creates the `EdenCPUThreadPool` (aka `ServerState::threadPool_`), a
CPU pool (12 threads as of Nov 2024, configurable via EdenConfig's
`core:eden-cpu-pool-num-threads`) for miscellaneous background tasks. These
threads handle post-mount initialization, prefetching, and post-retry logic.

The queue to the miscellaneous CPU pool must be unbounded because, if it could
block, there could be a deadlock between it and the other pools. To use a
bounded queue and avoid deadlocks we'd have to guarantee anything that runs in
the miscellaneous CPU pool can then never block on the retry again. (Adding to
the retry queue blocks if it's full.)

## Blocking

In general, we try to avoid blocking on other threads. The only places we ought
to block are talking to the filesystem and contending on locks. (Today, as
mentioned above, we will block if inserting into the retry queue is full.)
