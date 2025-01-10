# SaplingBackingStoreStats

1. `Duration get{xxx}{"store.sapling.get_{xxx}_us"}` :

Duration of the whole get xxx (blob, blobmetadata, tree, treemetadata)
SaplingBackingStore::get{xxx} in Microsecond. This includes looking in local
first then if not found prepare the request, enqueue the request and then mark
it as finished when it is fulfilled.

1. `Duration fetch{xxx}{"store.sapling.fetch_{xxx}_us"}` :

Duration of fetching xxx (blob, blobmetadata, tree, treemetadata) requests from
the network in Microsecond.

1. `Duration getRootTree{"store.sapling.get_root_tree_us"}` :

Duration of getting a Root Tree from the Backing Store in Microsecond.

1. `Duration importManifestForRoot{"store.sapling.import_manifest_for_root_us"}`
   :

Duration of getting a manifest for Root from the Backing Store in Microsecond.

1. `Counter fetch{xxx}Local{"store.sapling.fetch_{xxx}_local"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) fetching locally from
hgcache

1. `Counter fetch{xxx}Success{"store.sapling.fetch_{xxx}_success"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) that fetch successfully
in the first try. (It could be local or remote)

1. `Counter fetch{xxx}Failure{"store.sapling.fetch_{xxx}_failure"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) that failed after retry.

1. `Counter fetch{xxx}RetrySuccess{"store.sapling.fetch_{xxx}_retry_success"}` :

Number of xxx (blob, tree) that fetch successfully in the retry. (It could be
local or remote)

1. `Counter getRootTreeLocal{"store.sapling.get_root_tree_local"}` :

Number of root trees fetching locally from Cache

1. `Counter getRootTreeSuccess{"store.sapling.get_root_tree_success"}` :

Number of root trees that fetch successfully in the first try. (It could be
local or remote)

1. `Counter getRootTreeFailure{"store.sapling.get_root_tree_failure"}` :

Number of root trees that failed after retry.

1. `Counter getRootTreeRetrySuccess{"store.sapling.get_root_tree_retry_success"}`
   :

Number of root trees that fetch successfully in the retry. (It could be local or
remote)

1. `Counter importManifestForRootLocal{ "store.sapling.import_manifest_for_root_local"}`
   :

Number of manifest for root fetching locally from Cache

1. `Counter importManifestForRootSuccess{"Store.sapling.import_manifest_for_root_success"}`
   :

Number of manifest for root that fetch successfully in the first try. (It could
be local or remote)

1. `Counter importManifestForRootFailure{"Store.sapling.import_manifest_for_root_failure"}`
   :

Number of manifest for root that failed after retry.

1. `Counter importManifestForRootRetrySuccess{"Store.sapling.import_manifest_for_root_retry_success"}`
   :

Number of manifests for root that fetch successfully in the retry. (It could be
local or remote)

1. `Duration prefetchBlob{"store.sapling.prefetch_blob_us"}` :

Duration of prefetching Blobs requests from BackingStore.

1. `Counter prefetchBlobLocal{"store.sapling.prefetch_blob_local"}` :

Number of Blobs prefetching locally from Cache

1. `Counter prefetchBlobSuccess{"store.sapling.prefetch_blob_success"}` :

Number of Blobs that prefetch successfully in the first try. (It could be local
or remote)

1. `Counter prefetchBlobFailure{"store.sapling.prefetch_blob_failure"}` :

Number of Blobs that failed in the prefetch after retry.

1. `Counter prefetchBlobRetrySuccess{"store.sapling.prefetch_blob_retry_success"}`
   :

Number of Blobs that prefetch successfully in the retry. (It could be local or
remote)

1. `Counter loadProxyHash{"store.sapling.load_proxy_hash"}` :

Count the number of times that a proxy hash gets loaded.
