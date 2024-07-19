# EdenFS exposed Sapling counters

These stats signals are recorded in the Sapling layers. However, they have
information that is needed on EdenFS debuggings. Currently we have the following
categories of these stats:

**A-** `scmstore.file.fetch.{xxx}.{yyy}.{zzz}`:

{xxx} can be `aux`, `indexedlog`, `lfs` which are the place that Sapling fulfill
the request

{yyy} can be `cache` or `local` which are the place that Sapling fulfill the
request.

`cache` is the hgcache which is the remote server's cache. The objects in cache
can be deleted anytime, because it is cache!

`local` are the files that are modified localy. The objects in local are
permanent, and cannot be deleted like cache.

- {zzz} = `keys` : `scmstore.file.fetch.{xxx}.{yyy}.keys`
  - The total number of files that are requested in any of those places
- {zzz} = `requests` : `scmstore.file.fetch.{xxx}.{yyy}.requests`
  - The total number of requests that are sent to any of those places. Each
    request has a batch of some keys.
- {zzz} = `hits` : `scmstore.file.fetch.{xxx}.{yyy}.hits`
  - The number of files that are fulfilled in any of those places
- {zzz} = `misses` : `scmstore.file.fetch.{xxx}.{yyy}.misses`
  - The number of files that are missed in any of those places
- {zzz} = `time` : `scmstore.file.fetch.{xxx}.{yyy}.time`
  - The total time that spend to fetch files from any of those places

**Note1** : Also, we have `scmstore.file.fetch.aux.cache.computed` which is the
number of computed aux data.

**Note2** : Only for `indexedlog` in addition to `file` we have `tree` counters
too. It shows the information (keys, requests, hits, misses, and time) for
directory requests.

**B-** `scmstore.{xxx}.fetch.edenapi.{yyy}`:

These are all the signals that are recorded when Sapling fulfills a {xxx} (file
| tree) request from Mononoke.

- {yyy} = `hits` : `scmstore.file.fetch.edenapi.hits`
  - The number of files that are fulfilled in Mononoke
- {yyy} = `keys` : `scmstore.file.fetch.edenapi.keys`
  - The total number of files that are requested from Mononoke
- {yyy} = `requests` : `scmstore.file.fetch.edenapi.requests`
  - The total number of requests that are sent to Mononoke. Each request has a
    batch of some keys.
- {yyy} = `time` : `scmstore.file.fetch.edenapi.time`
  - The total time that spend to fetch files from Mononoke

**C-** `eden.edenffi.ffs.{xxx}` :

FileteredFS backing store sends a request to Sapling to check the filter for a
request object. Sapling collects the following stats which are reported in
EdenFS counters:

- {xxx} = `lookups` : The total number of filter object requests send to Sapling
- {xxx} = `lookup_failures` : The total number of filter object requests which
  are failed
- {xxx} = `invalid_repo` : The total number of filter object requests which has
  invalid repo
- {xxx} = `repo_cache_hits` : The total number of filter object requests that
  don't have to recreate the repo object. It is already in the cache
- {xxx} = `repo_cache_misses` : The total number of filter object requests that
  have to recreate the repo object. It is not available in the cache

**Note:** This
[commit](https://github.com/facebook/sapling/commit/d5392b1e72f0443a2cb0f4e76d19a58d615cb27b)
is a good examples of how to add counters to Sapling.
