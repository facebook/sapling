# Git LFS representation

This docs covers the internal data representaion for Git LFS files in Mononoke.

## Why bother?

Vanilla Git is not aware of the files being LFS or not. In Git all the handling is done client-side
by git-lfs filters and the server only receives and servers the pointer to LFS files. Mononoke could
follow the suit and still be a Git-compatible server. But this approach has some unwanted properties:

 * All queries and access to Git LFS files via different surfaces (like SCS API) yield only the pointer contents.
 * All rollup statistics and hashes are based on the pointer size and not on actual checkout size
   which makes them useless for estimating the actual end user UX.
 * Mononoke's internal data integrity checks wouldn't cover LFS file contents.
 * All Cross-Repo syncs would be syncing pointers rather than files: in case of syncing from Git to
   Sapling that would mean swapping the file for not-so-useful pointer content. In case of syncing
   to other Git repo that wouldn't gurantee the other repo being LFS-enabled and actual file contents
   being replicated.

## It's optional

If the above downsides are not a problem for you, or if you're using some external LFS server rather
than Mononoke internal one you don't need to worry: this feature is entirely optional and if diabled
LFS pointers are treated just like any other files.

## Definitions
 * *Git LFS pointer* - a small checked-in pointer, that points to file contents on Git LFS server.
 * *Full file contents* - the actual file contents the pointer points to

## Git LFS pointer interpretation

When translating from Git data model to Mononoke's internal model (Bonsai) we pattern match the file contents.
If the contents look like LFS then:
 * we ensure that both, pointer and full file contents are uploaded to Mononoke's blobstore
 * we use the full file contents as the file contents in bonsai
 * we set a special attribute on bonsai's file change to indicate that the file should be represented as LFS
   on Git's side

## Representation

For each file that the commit changes in bonsai changeset we hold a `FileChange` struct that has
special optional `git_lfs` field:

```
struct FileChange {
  ...
  5: optional GitLfs git_lfs;
}

struct GitLfs {
  1: optional id.ContentId non_canonical_pointer_content_id;
}
```

Just mere presence of this structure is enough to get the file changes represented as Git LFS
pointer when served using Git data formats.

Leaving this datastructure entirely empty is recommended when creating new commits originating from
outside of Git and the ones created by canonical Git-LFS client.  But if the commit was created by
by rougue client and the pointer is not exactly byte-for-byte equal to what Git-LFS or mononoke
would create then we upload.

By canonical pointer we mean one that looks **exactly** like:
```
version https://git-lfs.github.com/spec/v1
oid sha256:4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393
size 12345
(ending \n)
```

If the Git LFS pointer interpretatin is disabled we just store the pointer and set the `git_lfs` field to `None`.

## It doesn't have to be consistent within a Mononoke repo

In some cases a single repo can have some pointers interpreted and some stored without resolving to
full contents. This is useful in cases where for some parts of the repo history the original
contents are lost (they're stored on external server so that's a possiblity) and for new contents we
want to leverage the pointer interpretation repo.

## Conclusion

This way of representing LFS isolates the Git-LFS just to Git LFS data format and allows other APIs
to not be aware of intricancies of Git and just access file contents in a consistent way.

-----

For more information of the Git-LFS data format see [official spec on GitHub](https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md).
