# Augmented Manifest

# Augmented Manifest format

Augmented Manifest is the key part of the content-addressed data model in Source Control

## What is the difference from the Sapling Manifest:

* A Sapling manifest entry is limited to a path and sha1 hash, referencing either a filenode or another Sapling manifest entry.
Notably, file metadata (content blake3, content sha1, copy info, and blob size) is not included in the manifest itself, but rather provided separately.


```
# root manifest entry
README.txt -> 4f8d7b9c2e5a3710686e0bb32c52e53844e76721, file type
src -> a9b8c7d6e5f4g3h2i1j0k9l8m7n6o5p4q3r2s1t
```

```
# manifest entry for src
foo.txt -> 2a3b4c5d6e7f8g9h0i1j2k3l4m5n6o, file type
bar.txt -> 8c7d6e5f4g3h2i1j0k9l8m7n6o5p
```

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

## Storage in Mononoke

* In Mononoke, Augmented Manifests are stored in a binary format, [specifically serialized thrift](https://www.internalfb.com/code/fbsource/[95f0848f732fb330970d48c0c350557f1f3f7472]/fbcode/eden/mononoke/mercurial/types/if/mercurial_thrift.thrift?lines=90), and are addressed using sapling manifest ids.
* To optimize storage and cache efficiency, Mononoke's Augmented Manifests employ a [sharded manifest](https://fb.workplace.com/groups/scm.mononoke/permalink/2371492546546640/) approach.
