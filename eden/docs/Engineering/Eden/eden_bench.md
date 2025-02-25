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
2. **Sapling Prefetch** (`sl`)
In this mode, we prefetch the entire dataset into the local Sapling Backing Store level cache (either SaplingCache or Local CASd cache for CASC) before running the crawl.
This approach helps isolate the benchmarking to scenarios where the Sapling Backing Store level caches are warm.
<br>
<br>
3. **Eden Prefetch** (`eden`)
Here, we prefetch the entire dataset into all layers of the EdenFS caches before crawling, while cleaning up kernel/page caches to ensure an accurate measurement. This mode is ideal for evaluating the overall caching performance in EdenFS.


To assess the impact of kernel cache warming on EdenFS performance, we execute each benchmark's crawling component in a series of three iterations: `cold` -> `warm` -> `hot`
* **Cold**: The initial run, where kernel caches are empty.
* **Warm**: The second run, where kernel caches have started to warm up.
* **Hot**: The final run, where kernel caches are fully warmed up.


