# Packable Blobstore "packblob"

## Overview
Packblob will be able to optionally pack multiple key/value pairs into one blob. Packblob takes a thrift approach to the "raw" vs "compressed" decision on `Blobstore::get()`,  rather than attempting double read (e.g. by dual keys), or using out of band information (e.g. key properties/extended attributes).

Conceptually an entire blobstore could be represented in one packblob value (although this would of course be too slow/large in practice for real world usage).

## Underlying blobstore features
Packblob will require that the underlying store has a hardlink-like facility where the underlying target key is not discoverable easily, so the target key is included in the packed form (this would be redundant if we assumed symlink style links). This is to minimise latency, as in-use high latency stores require no extra round trip for hardlink-like links.

For lower latency stores if they have no link facility, it could be emulated with a symlink like blob payload.

## Extensibility
Packblob is designed to be extensible by the addition of new thrift union variants.

Packblob will sit between the underlying storage (sqlblob, manifoldblob etc) and the multiplexblob and caching layers. It is important that all blobs in a given (store,repo) tuple have the packblob StorageEnvelope wrapper so that on read the discriminant can tell the reader the returned values storage layout.

## Cache interaction
In order to cache the packed blobs packblob may also talk to caching layers directly. The envisioned interaction is:

```
storage (e.g. mysql) <-> storageblob (e.g. sqlblob) <-> packblob <-> multiplexedblob <-> cacheblob <-> prefixblob <-> mononoke blobrepo
                                |                                                           |
                         <cache packed form>                                       <cache unpacked data>
```

## Multiplex interaction
As the above diagram shows, packblob is expected to be below multiplexedblob in the stack,  so it is possible to have a multiplex consisting of one or more unpacked stores plus one or more packblob stores.

## Compression
Packblob will support compression of both single independent values, and of packed values.   The layout of these will be up to the packer,  initial testing has shown that using packed Zstd deltas where a blob version is the dictionary and the other blobs in the pack are compressed referencing it is efficient for Mononoke data.
