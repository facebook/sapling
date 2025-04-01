# Eden Bench - Benchmarking Script

**[Eden Bench](https://www.internalfb.com/code/fbsource/fbcode/eden/fs/scripts/facebook/eden_bench.sh) is EdenFS's crawling benchmarking script.**

This benchmarking script covers two primary types of crawling:

* **Repo Content Crawling**: Crawling the actual content within a repository.
* **Repo Metadata Crawling** (Directories Walk): Crawling the metadata, such as directory structures.

For the Repo Metadata Crawling, we support two interfaces:
* **Regular Filesystem API**: Utilizing the standard filesystem API for crawling.
* **Eden Thrift API**: Leveraging the Eden Thrift API for more efficient crawling.

We offer three distinct modes for benchmark runs:

1. **All Caches Cold** (`no_prefetch`)
This mode simulates a scenario where all caches are empty, allowing us to measure the impact of remote storage latencies and write I/O on crawling performance.
<br>
<br>
2. **Sapling Prefetch** (`sl_prefetch`)
In this mode, we prefetch the entire directory into the local Sapling Backing Store level cache (either Sapling Cache or Local CASd cache if CASC configured) before running the crawl.
This approach helps isolate the benchmarking to scenarios where the Sapling Backing Store level caches are warm.
<br>
<br>
3. **Eden Prefetch** (`eden_prefetch`)
Here, we prefetch the entire directory into all layers of the EdenFS caches before crawling, while cleaning up kernel/page caches to ensure an accurate measurement. This mode is ideal for evaluating the overall caching performance in EdenFS.


To assess the impact of kernel cache warming on EdenFS performance, we execute each benchmark's crawling component in a series of three iterations: `cold` -> `warm` -> `hot`
* **Cold**: The initial run, where kernel caches are empty.
* **Warm**: The second run, where kernel caches have started to warm up.
* **Hot**: The final run, where kernel caches are fully warmed up.

[All available benchmarks](https://www.internalfb.com/code/fbsource/[16dae41e91d3704edd3993bbc8db372c2fac7993]/fbcode/eden/fs/scripts/facebook/eden_bench.sh?lines=6)

## Bulk runs

It is possible to run several benchmarks at ones.

```
# run all repo content crawling benchmarks
/usr/local/libexec/eden/eden_bench.sh 5808178a971157999fb581af1c59ade724d66f8e all_content_crawling
```

For the Repo Metadata Crawling, it is recommended to use a large directory like the entire `www`

```
export BENCHMARK_REPO_PATH=www
```

```
# run all repo metadata crawling benchmarks
/usr/local/libexec/eden/eden_bench.sh 5808178a971157999fb581af1c59ade724d66f8e all_metadata_crawling
```

To customize the number of runs for averaging results, please utilize the following option:
```
export BENCHMARK_NUM_RUNS=10
```


## Measurements and Baseline

We suggest primarily using the "MB/s bandwidth metric" for comparing **Repo Content Crawling** benchmarks and "directories per second" for **Repo Metadata Crawling** benchmarks. Additionally, we provide the total duration for reference.
As a baseline, we recommend utilizing measurements collected from the native file system. These measurements are available when the BENCHMARK_ENABLE_ON_DISK_RG environment variable is enabled.

Example:

```
BENCHMARK_ENABLE_ON_DISK_RG=1 /usr/local/libexec/eden/eden_bench.sh 5808178a971157999fb581af1c59ade724d66f8e sl_prefetch
```

## Measurements under Memory Pressure

To make the run more similar to a production environment, consider the following strategies:
* Prefetch an extensive cache, rather than just the data accessed during the current crawling session.

```
export BENCHMARK_PREFETCH_REPO_PATH="www"
```
* Execute your benchmark within the context of a systemd unit.

```
systemd-run --scope --user -p MemorySwapMax=0 -p MemoryMax=7G -- eden_bench <all args>
```


## Check Configuration

If your host is enrolled to use the Mononoke dogfooding tier, it is recommended to disable it as it may impact performance.

```
$ hg config | grep edenapi.url
```

`https://mononoke-dogfooding.internal.tfbnw.net/edenapi/` is the Mononoke dogfooding tier

Add these lines to your `~/.hgrc`

```
[edenapi]
url=https://mononoke.internal.tfbnw.net/edenapi/
```

## Results for Automation

The results in json and cvs formats are available here:

```
/tmp/benchmarks/logs/eden_perf/report.csv
/tmp/benchmarks/logs/eden_perf/report.json
```

## Locally Built EdenFS

Easily collect benchmarking results with EdenFS built from local code by using the following option:

```
export BENCHMARK_ENABLE_LOCAL_DEV_EDEN=1
```

## Locally Built Sapling and CASd

Apart from EdenFS, it is also possible to build Sapling/CASd locally, or even all the binaries.
```
export BENCHMARK_ENABLE_LOCAL_DEV_SAPLING=1 # Sapling
export BENCHMARK_BUILD_CASD_FROM_SOURCE=1   # CASd
export BENCHMARK_ENABLE_ALL_LOCAL_BUILDS=1  # all binaries: Sapling, CASd and EdenFS
```


## EdenFS Profiling

Easily collect a CPU profile for the EdenFS daemon feature by using the following option:

```
export BENCHMARK_ENABLE_EDENFS_PROFILING=1
```

## Customizing Ripgrep Crawling Concurrency

To adjust the concurrency of ripgrep crawling, utilize the following option:
```
export BENCHMARK_CONCURRENCY_RG=32
```

## CASC configuration

By default, the benchmarking script employs the configuration of the host where it is executed. 
As a result, our ongoing CASC rollout may impact the results significantly. 
To address this, we provide helpers that allow you to manually turn CASC on or off.
```
export BENCHMARK_ENABLE_CAS=1
```
```
export BENCHMARK_ENABLE_CAS=0
```

## ❤️‍🩹 Recovering from a Bad State:

Eden bench script uses a **separate** EdenFS daemon, Sapling backing repo and `fbsource` repo checkout.
Your current `fbsource` repo checkout remains intact while running benchmarks and your work is not disrupted.

If your benchmarking repository has become corrupted or EdenFS is stuck:
1. kill the corresponding EdenFS daemon if running (`ps ax | grep /usr/local/libexec/eden/edenfs | grep benchmarking`)
2. force umount the repo (`sudo umount -lf  /data/users/$USER/benchmarking/__test_fbsource_prod/` or dev)
3. remove the entire directory (`/data/users/$USER/benchmarking/`)


## EdenFS configuration of the CAS client (former RE client)

Sapling's configuration settings reflect most of the CAS client options, enabling EdenFS to support various fetching modes.
Use config section `[cas]` to override the configs when required (we would recommend to put them to `~/.hgrc.fbsource.override`).

Note that although these settings are configured through Sapling configs, they exclusively impact EdenFS and do not influence Sapling's behavior (`sl`).
Sapling uses the thin client (also known as external client), that performs all the fetches/prefetches via Wdb CASd (`download_digests_into_cache` or `download` APIs are used).

| Config | CAS | Description | RO CACHE|
|----------|----------|----------|----------|
|`shared_cache.local.small_files=true`|`remote_cache_config.small_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC`| *This is the default mode for the dogfooding clients.* Small files are fetched directly and synced to Wdb CASd via WAL files. Large files are fetched through Wdb CASd| Used|
|`shared_cache.local.all_files=true`|`remote_cache_config.small_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;` `remote_cache_config.large_files = RemoteFetchPolicy::LOCAL_FETCH_WITH_SYNC;`|ALL files are fetched directly and synced to Wdb CASd via WAL files| Used|
|`shared_cache.local.small_files=false` and `shared_cache.local.all_files=false`|`remote_cache_config.small_files = RemoteFetchPolicy::REMOTE_FETCH;` `remote_cache_config.large_files = RemoteFetchPolicy::REMOTE_FETCH;`|All files are fetched via Wdb CASd|Used|
|`use-shared-cache=false`|`RemoteCacheConfig` is not enabled|Enables the Rich Client mode, where the shared cache is not used| Not used|




<br>

Please, check out more options available here: [source](https://www.internalfb.com/code/fbsource/[16dae41e91d3704edd3993bbc8db372c2fac7993]/fbcode/eden/fs/scripts/facebook/eden_bench.sh?lines=143-184)

