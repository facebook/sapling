# SaplingBackingStoreStats

1. `Duration get{xxx}{"store.sapling.get_{xxx}_us"}` :

Duration of the whole get xxx (blob, blobmetadata, tree, treemetadata)
SaplingBackingStore::get{xxx} in Microsecond. This includes looking in local
first then if not found prepare the request, enqueue the request and then mark
it as finished when it is fulfilled.

2. `Duration fetch{xxx}{"store.sapling.fetch_{xxx}_us"}` :

Duration of fetching xxx (blob, blobmetadata, tree, treemetadata) requests from
the network in Microsecond.

3. `Duration getRootTree{"store.sapling.get_root_tree_us"}` :

Duration of getting a Root Tree from the Backing Store in Microsecond.

4. `Duration importManifestForRoot{"store.sapling.import_manifest_for_root_us"}`
   :

Duration of getting a manifest for Root from the Backing Store in Microsecond.

5. `Counter fetch{xxx}Local{"store.sapling.fetch_{xxx}_local"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) fetching locally from
hgcache

6. `Counter fetch{xxx}Remote{"store.sapling.fetch_{xxx}_remote"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) fetching remotely from
the network (EdenAPI)

7. `Counter fetch{xxx}Success{"store.sapling.fetch_{xxx}_success"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) that fetch successfully
in the first try. (It could be local or remote)

8. `Counter fetch{xxx}Failure{"store.sapling.fetch_{xxx}_failure"}` :

Number of xxx (blob, blobmetadata, tree, treemetadata) that failed in the first
fetch try.

9. `Counter fetch{xxx}RetrySuccess{"store.sapling.fetch_{xxx}_retry_success"}` :

Number of xxx (blob, tree) that fetch successfully in the retry. (It could be
local or remote)

10. `Counter fetch{xxx}RetryFailure{"store.sapling.fetch_{xxx}_retry_failure"}`
    :

Number of xxx (blob, tree) that failed in the fetch retry.

11. `Counter getRootTreeLocal{"store.sapling.get_root_tree_local"}` :

Number of root trees fetching locally from Cache

12. `Counter getRootTreeRemote{"store.sapling.get_root_tree_remote"}` :

Number of root trees fetching remotely from Sapling BackingStore

13. `Counter getRootTreeSuccess{"store.sapling.get_root_tree_success"}` :

Number of root trees that fetch successfully in the first try. (It could be
local or remote)

14. `Counter getRootTreeFailure{"store.sapling.get_root_tree_failure"}` :

Number of root trees that failed in the first fetch try.

15. `Counter getRootTreeRetrySuccess{"store.sapling.get_root_tree_retry_success"}`
    :

Number of root trees that fetch successfully in the retry. (It could be local or
remote)

16. `Counter getRootTreeRetryFailure{"store.sapling.get_root_tree_retry_failure"}`
    :

Number of root trees that failed in the fetch retry.

17. `Counter importManifestForRootLocal{ "store.sapling.import_manifest_for_root_local"}`
    :

Number of manifest for root fetching locally from Cache

18. `Counter importManifestForRootRemote{"Store.sapling.import_manifest_for_root_remote"}`
    :

Number of manifest for root fetching remotely from Sapling BackingStore

19. `Counter importManifestForRootSuccess{"Store.sapling.import_manifest_for_root_success"}`
    :

Number of manifest for root that fetch successfully in the first try. (It could
be local or remote)

20. `Counter importManifestForRootFailure{"Store.sapling.import_manifest_for_root_failure"}`
    :

Number of manifest for root that failed in the first fetch try.

21. `Counter importManifestForRootRetrySuccess{"Store.sapling.import_manifest_for_root_retry_success"}`
    :

Number of manifests for root that fetch successfully in the retry. (It could be
local or remote)

22. `Counter importManifestForRootRetryFailure{"Store.sapling.import_manifest_for_root_retry_failure"}`
    :

Number of manifests for root that failed in the fetch retry.

23. `Duration prefetchBlob{"store.sapling.prefetch_blob_us"}` :

Duration of prefetching Blobs requests from BackingStore.

24. `Counter prefetchBlobLocal{"store.sapling.prefetch_blob_local"}` :

Number of Blobs prefetching locally from Cache

25. `Counter prefetchBlobRemote{"store.sapling.prefetch_blob_remote"}` :

Number of Blobs prefetching remotely from Sapling BackingStore

26. `Counter prefetchBlobSuccess{"store.sapling.prefetch_blob_success"}` :

Number of Blobs that prefetch successfully in the first try. (It could be local
or remote)

27. `Counter prefetchBlobFailure{"store.sapling.prefetch_blob_failure"}` :

Number of Blobs that failed in the first prefetch try.

28. `Counter prefetchBlobRetrySuccess{"store.sapling.prefetch_blob_retry_success"}`
    :

Number of Blobs that prefetch successfully in the retry. (It could be local or
remote)

29. `Counter prefetchBlobRetryFailure{"store.sapling.prefetch_blob_retry_failure"}`
    :

Number of Blobs that failed in the prefetch retry.

30. `Counter loadProxyHash{"store.sapling.load_proxy_hash"}` :

Count the number of times that a proxy hash gets loaded.
