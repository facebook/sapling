# Eden's Threading Strategy

There are `fuseNumThreads` (defaults to 16 as of Dec 2017) that block on reading
the FUSE socket.  The reason we do blocking reads is to avoid two syscalls on an
incoming event: an epoll wakeup plus a read.  Note that there is a FUSE socket
per mount.  So if you have 3 mounts, there will be `3*fuseNumThreads` threads.

The FUSE threads generally do any filesystem work directly rather than putting
work on another thread.

The Thrift server uses `thrift_num_workers` IO threads (defaults to ncores).
We don't change the default number (ncores) of Thrift CPU threads.  The
IO threads receive incoming requests, but serialization/deserialization and
actually handling the request is done on the CPU threads.

There is another pool of (8 as of Dec 2017) threads on which the HgBackingStore
farms work out to (blocking) a Sapling retry processes.  Because importing from
Sapling is high-latency and mostly blocking, we avoid doing any post-import
computation, so it's put into the following pool.  Note that each HgBackingStore
has its own pool, and there is one HgBackingStore per underlying Sapling
repository.

Eden also creates a CPU pool (12 threads as of Dec 2017) for miscellaneous
background tasks.  These threads handle post-mount initialization, prefetching,
and post-retry logic.

The queue to the miscellaneous CPU pool must be unbounded because, if it could
block, there could be a deadlock between it and the other pools.  To use a
bounded queue and avoid deadlocks we'd have to guarantee anything that runs in
the miscellaneous CPU pool can then never block on the retry again.  (Adding
to the retry queue blocks if it's full.)

## Blocking

In general, we try to avoid blocking on other threads.  The only places we ought
to block are talking to the filesystem and contending on locks.  (Today, as
mentioned above, we will block if inserting into the retry queue is full.)
