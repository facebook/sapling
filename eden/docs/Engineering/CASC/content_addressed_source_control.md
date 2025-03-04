# CASC

CASC stands for **Content-Addressed-Source-Control**, the project aimimg to utilise CAS in Source Control to solve a class of problems, initially for the `fbsource` megarepo.

* **Sapling Cache Issues**:
    * Inefficient eviction strategy.
    * Susceptible to data corruption because it doesn’t do a full recursive validation.
    * Troubleshooting is complicated, particularly when dealing with issues like poor performance under high memory pressure.
    * Local Sapling Cache can not be transparently shared on **OnDemand** due to security concerns causing them to build and maintain full repo prefetch via [hgcache updater](https://docs.google.com/document/d/1IM3q-sujxcywCbqSdpfIdWn0aHtOkTI8m1bNXvv8m1o) for the www repo. Full repo prefetch can not be scaled.

* **Caches Hierarchy and Engineering Time**: 
    * Current Source Control caching flow is excessive and requires lots of investment to scale

    **EdenFs Kernel caches =>  EdenFs In-Memory caches => EdenFS RocksDB caches => Sapling Cache (hgcache) => Mononoke In-Memory cache => Memcache => Hedwig => Manifold**

Integrating with CAS enables us to shift our focus towards other Source Control challenges.

* **Mononoke Overloads:**
    * Mononoke is not well optimised to act as a fetching service, currently serving up to [4M blobs per second](https://fburl.com/scuba/mononoke_test_perf/mexgp363), trees and files combined (using **781 T1** machines).
In reality, its optimal performance relies on the assumption that fetches are not well-batched (by utilizing consistent routing on proxygen). This is often true for traffic originating from EdenFS fuse but not applicable to EdenFS Thrift traffic (**Eden allows to fetch data via thrift over UDS bypassing file system**) or Eden prefetch.
However, `eden prefetch` has beein gaining popularity as a solution to mitigate the issue of accumulating remote fetch latencies resulting from sequential fuse fetches, that causes poor performance for the tools, especially hack-based, and longer TTS (*time to signal*) for user's DIFFs.
Any spike in the amount of well-batched traffic, coming typically from prefetching, is a common cause of Mononoke SEVs.

* **Eden Light**:

By utilizing CASC, the memory footprint of individual EdenFS daemons on [SCM on RE](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/repo_support_on_remote_execution) is significantly reduced, thereby enabling the allocation of multiple workers on a single host and ultimately enhancing platform efficiency.

* **Local Cache Hit Rate**

The Local Cache Hit Rate is anticipated to surpass that of the Sapling Cache on OnDemand and Sandcastle, resulting in lower end-to-end latency for EdenFS.

## Key feature of CAS:


* Top-class local "persistent caches" provided by the CAS team:
    * A combination of in-memory LMDB cache and on-disk storage.
    * Sophisticated eviction and space reclamation strategies to optimize cache utilization.
    * Implementation of bloom filters to reduce unnecessary lookups for missing blobs.
    * The cache is stored on a physical host and shared among users (OnDemand)/tw containers/sandcastle jobs/actions (on [SCM on RE](https://www.internalfb.com/wiki/Source_Control/Engineering/Repo_Support_On_Remote_Execution/repo_support_on_remote_execution) platform), while the Wdb CAS daemon ensures its integrity and handles write operations.
* ZWG frontend for ZippyDB storage provides low latency caching.
* Content hashes are validated ensuring no data corruption.
* Leveraging Hedwig's peer-to-peer capabilities to create a scalable network for large blob distribution, minimizing storage traffic.


## Requirements:

* To fulfill content-addressed data requirement for the CAS storage, a new data model was developed for EdenFS and Sapling, with Augmented Manifests playing a crucial role. For more information, please refer to the dedicated page: https://www.internalfb.com/wiki/Source_Control/Engineering/CASC/augmented_manifest


## Persistent Local Caches for On Demand:

Why **Persistent Caches** on On Demand are important?

The lifetime of a repository on On Demand is comprised of the preparation cycle and user session, which cannot exceed **18 hours**. 
This duration also applies to the Sapling Cache COW mount data lifetime, EdenFS daemon lifetime and the repo checkout lifetime. 
In the absence of prefetched Sapling Cache and with the use of resource-intensive tools like meerkat, it implies that most of the repository's data (such as www) is refetched at least daily on every host from scratch. 
Consequently, this would result in an unsustainable load on Mononoke, our Source Control backend.

Persistent Caches, of CASC, would allow to deprecate the expensive full (www) repo prefetching and significanly simplify repo cloning mechanisms for Developer Environments.


![](px/6HWvs)

