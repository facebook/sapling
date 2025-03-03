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
/usr/local/libexec/eden/eden_bench.sh 5808178a971157999fb581af1c59ade724d66f8e all_ripgrep
```

For the Repo Metadata Crawling, it is recommended to use a large directory like the entire `www`

```
export BENCHMARK_REPO_PATH=www
```

```
# run all repo metadata crawling benchmarks
/usr/local/libexec/eden/eden_bench.sh 5808178a971157999fb581af1c59ade724d66f8e all_readdir
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

<br>

Please, check out more options available here: [source](https://www.internalfb.com/code/fbsource/[16dae41e91d3704edd3993bbc8db372c2fac7993]/fbcode/eden/fs/scripts/facebook/eden_bench.sh?lines=143-184)

