# Dynamic Counters

The values of dynamic counters are computed and updated on-demand whenever they
are requested by a monitoring tool or API. When a monitoring tool or API
requests the value of one of these counters (e.g., using the fb303 command-line
client or the `getCounters()` Thrift method), the Facebook Base library (also
known as fb303) will call the corresponding lambda function that was registered
with the counter. This lambda function will then call the appropriate method to
retrieve the current value of the counter.

Most of these counters register their call-back functions in `EdenServer.cpp`
with the following code:

```
auto counters = fb303::ServiceData::get()->getDynamicCounters();
counters->registerCallback(counterName, lambdaFunction);
```

All of these counters should be unregistered on the deconstruction methods.

```
counters->unregisterCallback(counterName);
```

### Note:

The frequency at which the counters are updated depends on how often they are
queried by monitoring tools or APIs. If the counters are not queried frequently,
their values may become stale or outdated.

## SaplingBackingStore dynamic Counter

Based on the `RequestStage` enum we have two sets of dynamic counters in the
BackingStore layer.

```
/**
* stages of requests that are tracked, these represent where any request
* is in the process (for example any request could be queued or live)
*/
enum RequestStage {
    // represents any request that has been requested but not yet completed
    // (request in this stage could be in the queue, live, or in the case
    // of sapling store imports fetching from cache)
    PENDING,
    // represents request that are currently being executed (in the case of
    // sapling imports, only those fetching data, this does not include
    // those reading from cache)
    LIVE,
};
```

1. `store.sapling.pending_import.{xxx}.count` and
   `store.sapling.pending_import.{xxx}.max_duration_us` : Show the number and
   max duration of the xxx (blob, blobmetadata, tree, prefetch) requests which
   are in the SaplingImportRequestQueue. This is the total time of waiting in
   the queue and being a live request.

2. `store.sapling.pending_import.count` and
   `store.sapling.pending_import.max_duration_us` : Show the number and max
   duration of all the objects(blob, prefetch blob, tree, blob metadata)
   requests which are in the SaplingImportRequestQueue. This is the total time
   of waiting in the queue and being a live request.

3. `store.sapling.live_import.{xxx}.count` and
   `store.sapling.live_import.{xxx}.max_duration_us` : Show the number and max
   duration of xxx (blob, blobmetadata, tree, prefetch) requests being fetched
   individually from the backing store. After sending a batch of requests to
   Sapling, only the failed requests will be sent individually to the backing
   store (getRetry functions). Therefore, these dynamic counters only get value
   when a retry happens.

4. `store.sapling.live_import.batched_{xxx}.count` and
   `store.sapling.live_import.batched_{xxx}.max_duration_us` : When
   SaplingBackingStore is preparing a batch of xxx (blob, blobmetadata, tree)
   request, it pairs the request with a watch list and starts a watch. This
   watch will stop when the request is fulfilled. Therefore, these dynamic
   counters show the number and max duration of batches of xxx (blob,
   blobmetadata, tree) requests right now are processing in backingstore.

5. `store.sapling.live_import.count` and
   `store.sapling.live_import.max_duration_us` : Show the number and max
   duration of all the object (blob, blob metadata, prefetch blob, tree)
   requests being fetched individually or in a batch from the backing store.

## FSChannel Dynamic Counters

1. `fs.task.count` : Count the number of tasks queued up for the
   fschannelthreads. We will monitor this when we unbound to see how much memory
   we are using and ensure that things are staying reasonable. This will also
   help inform how many pending requests we should allow max.

## inodeMap Dynamic counters

1. `inodemap.{mountBasename ex. fbsource or www}.loaded` : Number of loaded
   inodes in the inodemap for an eden mount. `eden stats` command will show
   these counters

2. `inodemap.{mountBasename ex. fbsource or www}.unloaded` : Number of unloaded
   inodes in the inodemap for an eden mount. `eden stats` command will show
   these counters

3. `inodemap.{mountBasename ex. fbsource or www}.unloaded_linked_inodes` : The
   number of inodes that we have unloaded with our periodic linked inode
   unloading. Periodic linked inode unloading can be run at regular intervals on
   any mount type. This is the periodic task to clean up the inodes that are not
   used recently.

4. `inodemap.{mountBasename ex. fbsource or www}.unloaded_unlinked_inodes` : The
   number of inodes that we have unloaded with our periodic unlinked inode
   unloading. Periodic unlinked inode unloading is run after operations that
   unlink lots of inodes like checkout on NFS mounts. This counter only has
   value in macOS. NFSv3 has no inode invalidation flow built into the protocol.
   The kernel does not send us forget messages like we get in FUSE. The kernel
   also does not send us notifications when a file is closed. Thus EdenFS can
   not easily tell when all handles to a file have been closed. More details on
   the summary of this
   [commit](https://github.com/facebook/sapling/commit/ffa558bf847c5be4adc82899a793f3996619f332)

## Journal Dynamic counters

1. `journal.{mountBasename ex. fbsource or www}.count` : Show the number of
   entry in Journal

2. `journal.{mountBasename ex. fbsource or www}.duration_secs` : Show how far
   back the Journal goes in seconds

3. `journal.{mountBasename ex. fbsource or www}.files_accumulated.max` : Show
   the maximum number of files that accumulated in Journal.

4. `journal.{mountBasename ex. fbsource or www}.memory` : Show the memory usage
   of the Journal.

## Fuse Dynamic counters

1. `fuse.{mountBasename ex. fbsource or www}.live_requests.count` : Show the
   number of live Fuse requests

2. `fuse.{mountBasename ex. fbsource or www}.live_requests.max_duration_us` :
   Show the maximum duration of Fuse live requests

3. `fuse.{mountBasename ex. fbsource or www}.pending_requests.count` : Show the
   number of Fuse pending requests

## Cache Dynamic counters

1. `blob_cache.memory` : Show the total size of items in blob cache

2. `blob_cache.items` : Count the number of items in blob cache

3. `tree_cache.memory` : Show the total size of items in tree cache

4. `tree_cache.items` : Count the number of items in tree cache
