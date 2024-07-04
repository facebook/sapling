ObjectStoreStats
===============

1. `Duration get{xxx}{"store.get_{xxx}_us"}` :

The whole duration of get{xxx} xxx (blob, blobmetadata, tree) in ObjectStore. Consider that ObjectStore can get Object from memory (MemoryCache), LocalStore (OndiskCache), or BackingStore


2. `Counter get{xxx}FromMemory{"object_store.get_{xxx}.memory"}` :

Count the number of xxx (blob, blobmetadata, tree) that are successfully obtained from MemoryCache. It doesn’t check the local store either.


3. `Counter get{xxx}FromLocalStore{"object_store.get_{xxx}.local_store"}` :

Count the number of xxx (blob, blobmetadata, tree) that are successfully obtained from LocalStore (OnDiskCache). It doesn’t hit the BackingStore.


4. `Counter get{xxx}FromBackingStore{"object_store.get_{xxx}.backing_store"}` :

Count the number of xxx (blob, blobmetadata, tree) that are obtained from BackingStore.


5. `Counter get{xxx}Failed{"object_store.get_{xxx}_failed"}` :

Count the number of xxx (blob, blobmetadata, tree) cannot be fetched.


6. `Counter getBlobMetadataFromBlob{"object_store.get_blob_metadata.blob"}` :

Count the number of BlobMetadata that cannot be obtained from BackingStore, but we obtained Blob and from Blob we found the BlobMetadata.


7. `Duration getRootTree{"store.get_root_tree_us"}` :

The whole duration of getRootTree in ObjectStore.


8. `Counter getRootTreeFromBackingStore{ "Object_store.get_root_tree.backing_store"}` :

Count the number of RootTree that are obtained from BackingStore.


9. `Counter getRootTreeFailed{"object_store.get_root_tree_failed"}` :

Count the number of RootTree cannot be fetched.
