# EdenFS Caching Flow

Traditionally, EdenFS's caching flow has followed a hierarchical structure, where caches are checked in a sequential manner.

## How the Caching Flow is different for CASC?

A few important differences include:

* **Optimized Cache Flow**: By leveraging knowledge of blob sizes in advance and [counting bloom filter](https://en.wikipedia.org/wiki/Counting_Bloom_filter) prediction, CASC's cache flow has been optimized to bypass unnecessary lookups, resulting in improved efficiency.
* **Key Feature**: The new setup boasts a local cache situated on the physical host, with containers and jobs granted direct read-only access through an internal handshake mechanism.
* **Deprecation of the Legacy Layer**: We can disable EdenFS's rocksdb caches.
* **Memory Footprint**: We can also disable EdenFS's in-memory caches for [SCM on RE](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/repo_support_on_remote_execution) by utilising the LMDB as the main in-memory cache,  which is essential for efficiently collocating multiple workers on a single host.

![](px/6J9dw)

