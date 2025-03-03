# Augmented Manifest

The manifest entry in Source Control is the data blob that describes the contents of the repository at a particular directory at a particular changeset ID.

**Augmented Manifest** is the key part of the content-addressed data model in Source Control

## What is the difference from the Sapling Manifest?

🌲🌲🌲🌲🌲🌲🌲🌲 **Sapling Manifest** 🌲🌲🌲🌲🌲🌲🌲🌲

* A Sapling Manifest entry is limited to a path and sha1 hash, referencing either a filenode or another Sapling manifest entry.
Notably, file metadata (content blake3, content sha1, copy info, and blob size) is not included in the manifest itself, but rather provided separately.


```
# root manifest entry
README.txt -> 4f8d7b9c2e5a3710686e0bb32c52e53844e76721, file type
src -> a9b8c7d6e5f4g3h2i1j0k9l8m7n6o5p4q3r2s1t
```

```
# manifest entry for src
foo.txt -> 2a3b4c5d6e7f8g9h0i1j2k3l4m5n6o, file type
bar.txt -> 8c7d6e5f4g3h2i1j0k9l8m7n6o5p, file type
```

🌳🌳🌳🌳🌳🌳🌳🌳 **Augmented Manifest** 🌳🌳🌳🌳🌳🌳🌳🌳
* An Augmented Manifest is uniquely identified by its **content-addressed blake3 hash**.
It employs a double-pointer tree structure to efficiently represent the directory hierarchy (content-addressed blake3 hash and Sapling manifest id (sha1) required to reconstruct Sapling manifests).
Unlike traditional Sapling manifests, file metadata (content blake3, content sha1, copy info header, and the blob size) is an integral part of each manifest entry.

```
# root manifest entry
README.txt -> sapling filenode id (sha1), file type, blake3 hash, content sha1, blob size, copy info header (if applicable)
src -> sapling manifest hash, augmented manifest hash, size of the augmented manifest blob
```

```
# manifest entry for src
foo.txt -> sapling filenode id (sha1), file type, blake3 hash, content sha1, blob size, copy info header (if applicable)
bar.txt -> sapling filenode id (sha1), file type, blake3 hash, content sha1, blob size, copy info header (if applicable)
```

## Serialization Format (CAS)

The serialization format consists of a custom text format with a header that provides version details (such as v1), the sapling manifest id of itself, its p1/p2 parents and "computed" sapling manifest id (if different).
Subsequent lines list entries for child files or directories.

```
v1 c47051a609786d217634958c6b945b30303df1e2 - 6d7bd6bfa69eb54cc13011f8d8bfae881cc470d6 -
UBloksCarbonFollowersTabControllerAutogen.php375db905cfab1c9ae4563feb3d6fc250a9135f18r b99ad034e97eae10551a64c50e9d368b3273ef2dd39b87044bd289c590af7afd 738 ed801158f8ffc57ffa589439402723a9101c8bcb -
UBloksCarbonFollowersTabControllerTypedQueryBuilder.phpc58d0d8cfeb33951748cf9d412ee32fbef808040r e7c2d063ec22f680207a8215ac8588349bb5ae9a3d8cfa1278a4b9f4115f1b9c 872 2362384a7fa5db742346a4e9f3332d6ccb651b8e -
UBloksCarbonFollowersTabControllerTypedRequest.php1699560a592d8f9f0f6dac9cc19fc9a3932845c2r f0c496ae3cb46e2cdf85a315a8ac8d0b627add77d0fe133e4f22fd3363b441c4 940 bd47c3c02ccd8423f33d8c7f546fbeafa1e10797 -
```

## Main properties of the Augmented Manifest format

* Augmented manifest entries can be stored in CAS using digests (blake3 hash + size)
* Augmented manifests entries can be converted to Sapling manifests entries with separate file/tree metadata.
* The text format enables bisecting for efficient searching and debugging

## Content-Addressed File Blobs
In the Sapling data model, files are identified by a filenodeid, which is a SHA1 hash of a file blob combined with filenode metadata prefix.
This metadata includes sapling copy info header and p1/p2 pointing to parents filenodes.
In contrast, the CASC data model differs in its approach. File blobs are now stored raw and addressed using a Blake3 hash and size of their content. The p1/p2 information is retrieved separately through the history Sapling Remote API endpoint when necessary, while the copy info header is stored within augmented trees.

The CASC data model facilitates an exact match between files and their cached blobs.

Two separate small Sapling Caches are utilized to store distinct mappings:
* The `/var/cache/hgcache/<repo>/indexedlogdatastore_aux cache` (auxiliary file metadata cache) stores the relationship between a filenode id and its corresponding blake3/size information (a digest).
* The `<backing repo>/.hg/store/manifests/treeaux cache` (auxiliary tree metadata cache) maintains the mapping between a Sapling tree id and an augmented manifest digest.



## Storage in Mononoke

* In Mononoke, Augmented Manifests are stored in a binary format, [specifically serialized thrift](https://www.internalfb.com/code/fbsource/[95f0848f732fb330970d48c0c350557f1f3f7472]/fbcode/eden/mononoke/mercurial/types/if/mercurial_thrift.thrift?lines=90), and are addressed using sapling manifest ids.
* To optimize storage and cache efficiency, Mononoke's Augmented Manifests employ the "sharded maps" data structure for sharding large manifest entries into smaller cacheable pieces.

[Sharded maps](https://www.internalfb.com/wiki/Source_Control/Mononoke/Design/Mononoke_Types/sharded_map/) is an optimized approach for storing manifest blobs in smaller pieces, which enables better caching for Mononoke and more efficient reads.

## Fetching Augmented Manifests

* From Mononoke (by sapling `hgid`)

Based on the `monad` tool:
```
 [🍊] → monad --prod blobstore --repo-name fbsource fetch hgaugmentedmanifest.sha1.c47051a609786d217634958c6b945b30303df1e2
Key: hgaugmentedmanifest.sha1.c47051a609786d217634958c6b945b30303df1e2
Ctime: 1720669778 (2024-07-10 20:49:38 -07:00)
Size: 483 (457 compressed)

v1 c47051a609786d217634958c6b945b30303df1e2 - 6d7bd6bfa69eb54cc13011f8d8bfae881cc470d6 -
UBloksCarbonFollowersTabControllerAutogen.php375db905cfab1c9ae4563feb3d6fc250a9135f18r b99ad034e97eae10551a64c50e9d368b3273ef2dd39b87044bd289c590af7afd 738 ed801158f8ffc57ffa589439402723a9101c8bcb -
UBloksCarbonFollowersTabControllerTypedQueryBuilder.phpc58d0d8cfeb33951748cf9d412ee32fbef808040r e7c2d063ec22f680207a8215ac8588349bb5ae9a3d8cfa1278a4b9f4115f1b9c 872 2362384a7fa5db742346a4e9f3332d6ccb651b8e -
UBloksCarbonFollowersTabControllerTypedRequest.php1699560a592d8f9f0f6dac9cc19fc9a3932845c2r f0c496ae3cb46e2cdf85a315a8ac8d0b627add77d0fe133e4f22fd3363b441c4 940 bd47c3c02ccd8423f33d8c7f546fbeafa1e10797 -
```

* From RE CAS (by augmented manifest's digest)

Based on the `frecli` tool:
```
 [🍋] → frecli cas download-blob da4f684352a8576eed04010c129421c63751a7d1779233088f7fbc91d4288dce:730
v1 ae83739b00f5169285a45554d643cd088f9fca39 - 81cf736cab03fa524ac04d36c25f6c6ef1d2c435 -
MeerkatBkSettingsPrivacyYouthDefaultSettingsAudienceSelectorAutogen.phpd71db74337ef07b0c3dc3bdb1b53bfc7faf5e7edr 0f2d131b59bb0bb8d5a49eda3b81c09a13463369d120614017f57e6cf4858840 767 7a97e79cb73fa4fb06c303b98a716f391313e6a4 AQpjb3B5OiB3d3cvZmxpYi9fX2dlbmVyYXRlZF9fL0Jsb2tzRGVyaXZlZEVsZW1lbnRBdXRvZ2VuTWVlcmthdFN0ZXAvc2luZ2xlX3NvdXJjZS94aHBfYmtfX3NldHRpbmdzX19wcml2YWN5X195b3V0aF9kZWZhdWx0X3NldHRpbmdzX19hdWRpZW5jZV9zZWxlY3Rvci9CbG9rc0Rlcml2ZWRFbGVtZW50QXV0b2dlbkFydGlmYWN0L01lZXJrYXRCa1NldHRpbmdzUHJpdmFjeVlvdXRoRGVmYXVsdFNldHRpbmdzQXVkaWVuY2VTZWxlY3RvckF1dG9nZW4ucGhwCmNvcHlyZXY6IDRkMzMwYTU1ODY3ZDk3YzI1ZWRkNDM3OTkxNzhiM2RkMmZjZDA1OTQKAQo=
```

## What is the difference from CAS TDirectory?

CAS uses its [own format](https://www.internalfb.com/code/fbsource/[12909a0b74fed2c846835c1a34022ae3556f3e4f]/fbcode/remote_execution/lib/if/common.thrift?lines=479) to represent the contents of a directory. The format is different and due to recursive hashing, it's not possible to directly convert an Augmented Manifest entry into a TDirectory, even if stored in CAS.

However, in the future, it will be possible to materialise a directory in CAS from an augmented manifest digest. 


