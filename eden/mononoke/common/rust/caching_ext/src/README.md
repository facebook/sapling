# Implementing caching in Mononoke

Mononoke has a utility module called caching_ext which is designed to handle the complexity of caching for you. However, there are quite a few moving parts that you have to get right in order to use it; this guide aims to explain how to implement caching, batching and parallel fetching using `caching_ext`.


### TL;DR (checklist)

1. Create a type to represent your cacheable data. If you only want positive caching (not negative), this only represents the data you cache. If you want negative caching (“this key had no value”), then it needs to contain an `Option` or other suitable way to indicate a successful negative fetch.
2. Derive [Abomonation](https://docs.rs/abomonation_derive/0.5.0/abomonation_derive/) and implement `caching_ext::MemcacheEntity` for this cacheable data item. Use Thrift Compact Protocol to implement MemcacheEntity if it’s non-trivial.
3. Create a struct to hold all the items needed for your cache (cachelib handlers, memcache clients, keygens etc)
4. Implement `caching_ext::EntityStore<MyDataType>` for your cache struct. You can use one cache struct for multiple caches if you wish - you just need multiple implementations of traits. Note while implementing cache_determinator that the final TTL can be twice what you return, as there are two underlying caches, and we can fill process-local cachelib from Memcache.
5. Implement `caching_ext::KeyedEntityStore<MyCacheKeyType, MyDataType>` for your cache struct. Again, you can implement this multiple times for different key-value pairs on the same cache, and you can implement this to cache the same value type under different keys.
6. Use `caching_ext::get_or_fill_chunked` (or `caching_ext::get_or_fill`) to fetch data through the cache, taking care of filling the cache for you.

With that done, you have a cache that contains your data item, and that automatically copies from Memcache to process-local cachelib, and does the minimum number of fetches from backing store (based on your supplied chunk size). It also ensures that fetches are as large as possible, given your chunk size, and will parallelise fetches for you (within your set limits) if there’s more than one chunk to fetch from the backing store.


## Detailed Guide

caching_ext takes care of copying data into and out of Memcache and per-task cachelib instances for you (filling cachelib from Memcache, and filling both cachelib and Memcache from your backing store); it also requests missing data from your backing store in chunks and with a user-chosen parallelism. You have to tell it how to cache your data, and you have to pass requests to the caches when required.


### Cacheable data

You cannot cache arbitrary Rust types. Instead, follow the process below to create a cacheable data item; note that the orphan rule for traits means that if you want to cache absence of a value, you’ll want a wrapper struct around `Option<Type>`. Similar applies if you want to cache something like MPath or other “common” types without further data - the orphan rule means you need a wrapper around the type.

* Create a type (struct, enum) that represents your cacheable data item.
* Implement `MemcacheEntity` for that type. This converts to and from Bytes and is used to put the item into Memcache - you must ensure that there is no detectable difference between the item you get back from `MemcacheEntity::deserialize` and one you fetch from the backing store.
   * If the item is non-trivial, Thrift Compact Protocol is a good choice for serializing and deserializing
   * You must ensure that you fail safe when deserializing something from the cache that isn’t in expected format. Your serialization format should be able to detect corruption from the cache and reject bad bytes.
* `#[derive(Abomonation)]` for that type; this enables it to go into Cachelib local caching. If this fails to derive, then you need to simplify your cacheable data type until it can derive sensibly, or make other things derive Abomonation so that everything works as intended.


### EntityStore

Once you have cacheable data, you can create a struct that implements trait `EntityStore` to tell `caching_ext` how to store and retrieve your data. With the exception of `cache_determinator`, this is fully covered by the documentation of the `EntityStore` trait.

`cache_determinator` takes a reference to a value, and decides whether it should be cached, and what TTL is appropriate. It’s important to note here that your cache_determinator will be called not only when the value has been retrieved from backing store, but also when the value has been retrieved from Memcache and the intent is to store it in local cachelib; if you have a maximum TTL in mind, then cache_determinator should never return more than half that value, so that in the worst case (fetched from backing store to Memcache and local cachelib just before a change, fetched by a different process from Memcache to cachelib just before the Memcache TTL expires), you still meet your target time.


You should always use `impl_singleton_stats!` to implement the `stats()` method - this macro takes a string to identify the cache use case (e.g. “blobstore” or “changesets”), prepends “mononoke.cache”, and then creates ODS statistics on the use of this cache, which can be used to determine how effective the cache is (how many cache hits for each of cachelib and memcache). It also takes care of ensuring that there’s only one set of stats in each process that uses the cache - rather than creating duplicate counters for every instantiation of your store.


### KeyedEntityStore

With `EntityStore` implemented, you can now implement `KeyedEntityStore` on the same struct. This trait does two things: it supplies the cachelib key string for a given user-defined key, which is also fed to the Memcache KeyGen to determine the Memcache key, and it implements get_from_db, which fetches values from your backing store (usually, but not always, a database).


The key type can be any Rust type, as long as you can implement `KeyedEntityStore` for it - it can be a pre-existing type (like `ChangesetId`), a tuple (e.g. of a hash and a path) or whatever’s appropriate to the data you’re caching.


`get_from_db` is given a set of keys to look up, and returns a map of keys to fetched values. If lookup of a value returns an error, then this function should fail by returning an error; otherwise, the map should map from key to value to return to the user. You can indicate that a value does not exist to cache by not inserting into this map - this allows you to create a cache that only caches found results, not absence. No cache entries will be created for values not in the return map.


Using the cache via `get_or_fill_chunked`

Once you have implemented all the needed parts, using the cache is a case of calling one of two free functions in `caching_ext`:

* `get_or_fill`
* `get_or_fill_chunked`

`get_or_fill` is a simple wrapper around `get_or_fill_chunked` for cases where chunking and parallelism are not needed. It simply asks for all requested keys to be fulfilled in a single chunk, with no parallel fetches.


`get_or_fill_chunked` does the real work. It takes your `KeyedEntityStore` that you implemented earlier, and a set of keys to fetch; it then efficiently fetches keys from the two caches and from get_from_db so that there is no over-fetching (no fetching from Memcache if cachelib has the data, no fetching from your backing store if Memcache has the data), and the caches are filled after it returns (filling in cachelib if the data comes from Memcache or your backing store, and filling in both Memcache and cachelib if your backing store provides the data).

You can then implement cached accesses to your backing store by calling `get_or_fill_chunked` and translating the return value to whatever type you need to return to your callers. A call to `get_or_fill_chunked` returns exactly the same values as get_from_db for your store, except that it uses the cache to avoid calling `get_from_db`, and to minimise the number of requests it does make if there are cache misses.


`get_or_fill_chunked` also has the ability to limit the batch size requested from your backing store in one call to `get_from_db` and to make multiple parallel calls to `get_from_db`, each of which can finish in any order. The chunking is useful if your query uses something like SQL’s IN, which result in timeouts if the set to check is large enough, while the parallel queries are useful if your backing store benefits from multiple in-flight queries. Note that if you have only one chunk of queries to make, then there is no parallelism; it is your responsibility to ensure that the chunk size and parallelism quantity make sense.


When doing chunked queries, `get_or_fill_chunked` builds the chunks from keys that missed in both caches; this means that if you query for 10,000 keys, of which 8,000 are cache hits in Memcache or cachelib, your chunk will only be built from the remaining 2,000 keys. Further, it builds maximally sized chunks (not even sized); if you set your chunk size to 399, then it will request 5 chunks of 399 keys, and then one chunk of 5 keys to get the remaining 2,000, rather than 6 chunks of 333 or 334 keys.


Parallelism is done via `buffer_unordered` and simply aims to keep as many chunks in flight as you requested. There’s no intelligence here like spawning tasks; instead, it’s assumed that you’ll do the spawning in `get_from_db` if it helps your fetches.
