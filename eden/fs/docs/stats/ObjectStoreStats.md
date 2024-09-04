# ObjectStoreStats

1. `Duration get{xxx}{"store.get_{xxx}_us"}` :

The whole duration of get{xxx} xxx (blob, blobmetadata, tree, treemetadata) in
ObjectStore. Consider that ObjectStore can get Object from memory (MemoryCache),
LocalStore (OndiskCache), or BackingStore

2. `Counter get{xxx}FromMemory{"object_store.get_{xxx}.memory"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) that are
successfully obtained from MemoryCache. It doesn’t check the local store either.

3. `Duration get{xxx}MemoryDuration{"object_store.get_{xxx}.memory_us"}` :

Duration of get{xxx} (blobmetadata, tree) from MemoryCache.

4. `Counter get{xxx}FromLocalStore{"object_store.get_{xxx}.local_store"}` :

Count the number of xxx (blob, blobmetadata, tree) that are successfully
obtained from LocalStore (OnDiskCache). It doesn’t hit the BackingStore.
**Note:** TreeMetadata is not stored in the local store, so the tree_metadata
counter doesn't exist. Also, local store is disabled for Blob, so this counter
is all time zero for Blob.

5. `Duration get{xxx}LocalStoreDuration{"object_store.get_{xxx}.local_store_us"}`
   :

Duration of get{xxx} (blobmetadata, tree) from LocalStore(OnDiskCache).
**Note:** TreeMetadata is not stored in the local store, and local strore is
disabled for Blob, so the tree_metadata and blob local strore duration doesn't
exist.

6. `Counter get{xxx}FromBackingStore{"object_store.get_{xxx}.backing_store"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) that are
obtained from BackingStore.

7. `Duration get{xxx}BackingStoreDuration{"object_store.get_{xxx}.backing_store_us"}`
   :

Duration of get{xxx} (blobmetadata, tree) from BackingStore. **Note:** As local
store is disabled for Blob and TreeMetadata, then we don't have a separate
duration for getBlob or getTreeMetadata from backing store. "store.get_blob_us"
and "store.get_treemetadata_us" are the duration of backing store.

8. `Counter get{xxx}Failed{"object_store.get_{xxx}_failed"}` :

Count the number of xxx (blob, blobmetadata, tree, treemetadata) cannot be
fetched.

9. `Counter getBlobMetadataFromBlob{"object_store.get_blob_metadata.blob"}` :

Count the number of BlobMetadata that cannot be obtained from BackingStore, but
we obtained Blob and from Blob we found the BlobMetadata. Note: TreeMetadata
cannot be computed locally, so the tree version of this counter does not exist.

10. `Duration getBlobMetadataFromBlobDuration{"object_store.get_blob_metadata.from_blob_us"}`
    :

Duration of get BlobMetadata from blob. In this case, BlobMetadata cannot be
obtained from BackingStore, but we obtained Blob and from Blob we found the
BlobMetadata.

11. `Duration getRootTree{"store.get_root_tree_us"}` :

The whole duration of getRootTree in ObjectStore.

12. `Counter getRootTreeFromBackingStore{ "Object_store.get_root_tree.backing_store"}`
    :

Count the number of RootTree that are obtained from BackingStore.

13. `Counter getRootTreeFailed{"object_store.get_root_tree_failed"}` :

Count the number of RootTree cannot be fetched.
