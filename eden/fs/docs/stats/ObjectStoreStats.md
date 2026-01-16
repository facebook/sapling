# ObjectStoreStats

1. `Duration get{xxx}{"store.get_{xxx}_us"}` :

The whole duration of get{xxx} xxx (blob, blobmetadata, tree, treemetadata) in
ObjectStore. Consider that ObjectStore can get Object from memory (MemoryCache) or
BackingStore

1. `Counter get{xxx}FromMemory{"object_store.get_{xxx}.memory"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) that are
successfully obtained from MemoryCache. It doesn’t check the local store either.

1. `Duration get{xxx}MemoryDuration{"object_store.get_{xxx}.memory_us"}` :

Duration of get{xxx} (blobmetadata, tree) from MemoryCache.

1. `Counter get{xxx}FromBackingStore{"object_store.get_{xxx}.backing_store"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) that are
obtained from BackingStore.

1. `Duration get{xxx}BackingStoreDuration{"object_store.get_{xxx}.backing_store_us"}`
   :

Duration of get{xxx} (blobmetadata, tree) from BackingStore. **Note:** As local
store is disabled for Blob and TreeMetadata, then we don't have a separate
duration for getBlob or getTreeMetadata from backing store. "store.get_blob_us"
and "store.get_treemetadata_us" are the duration of backing store.

1. `Counter get{xxx}Failed{"object_store.get_{xxx}_failed"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) cannot be
fetched.

1. `Counter getBlobMetadataFromBlob{"object_store.get_blob_metadata.blob"}` :

Count the number of BlobMetadata that cannot be obtained from BackingStore, but
we obtained Blob and from Blob we found the BlobMetadata. Note: TreeMetadata
cannot be computed locally, so the tree version of this counter does not exist.

1. `Duration getBlobMetadataFromBlobDuration{"object_store.get_blob_metadata.from_blob_us"}`
    :

Duration of get BlobMetadata from blob. In this case, BlobMetadata cannot be
obtained from BackingStore, but we obtained Blob and from Blob we found the
BlobMetadata.

1. `Duration getRootTree{"store.get_root_tree_us"}` :

The whole duration of getRootTree in ObjectStore.

1. `Counter getRootTreeFromBackingStore{ "Object_store.get_root_tree.backing_store"}`
    :

Count the number of RootTree that are obtained from BackingStore.

1. `Counter getRootTreeFailed{"object_store.get_root_tree_failed"}` :

Count the number of RootTree cannot be fetched.
