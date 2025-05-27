  $ setconfig push.edenapi=true
  $ setconfig scmstore.cas-mode=on

First sanity check eagerepo works as CAS store
  $ newclientrepo client1 server
  $ echo "A  # A/dir/file = contents" | drawdag

Remote doesn't know about commit yet.
  $ hg debugcas -r $A dir/file
  file path dir/file, node 755659c90eeec95dba978cb076b6aab1a02fb313, digest CasDigest { hash: Blake3("7fdd58185ed7a18ef36521aa4f017af4ad6b79ecd080030f874db1faa20a26f6"), size: 8 }, not found in CAS

  $ hg push -qr $A --to main --create

Now remote knows about our data.
  $ hg debugcas -r $A dir/file
  file path dir/file, node 755659c90eeec95dba978cb076b6aab1a02fb313, digest CasDigest { hash: Blake3("7fdd58185ed7a18ef36521aa4f017af4ad6b79ecd080030f874db1faa20a26f6"), size: 8 }, contents:
  contents

Can also fetch tree data.
  $ hg debugcas -r $A ""
  tree path , node 7cdcd3e44b0bbf75d7f9e972890e8c0d4f16e231, digest CasDigest { hash: Blake3("74e68a61d720d2c1485916f740d3b5be75468062e1892f4f24e83d0f3ddd559b"), size: 319 }, contents:
  AugmentedTree {
      hg_node_id: HgId("7cdcd3e44b0bbf75d7f9e972890e8c0d4f16e231"),
      computed_hg_node_id: None,
      p1: None,
      p2: None,
      entries: [
          (
              PathComponentBuf(
                  "A",
              ),
              FileNode(
                  AugmentedFileNode {
                      file_type: Regular,
                      filenode: HgId("005d992c5dcf32993668f7cede29d296c494a5d9"),
                      content_blake3: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"),
                      content_sha1: Sha1("6dcd4ce23d88e2ee9568ba546c007c63d9131c1b"),
                      total_size: 1,
                      file_header_metadata: None,
                  },
              ),
          ),
          (
              PathComponentBuf(
                  "dir",
              ),
              DirectoryNode(
                  AugmentedDirectoryNode {
                      treenode: HgId("1d12614b1101689fa2c31b749d0c17fa9ad10564"),
                      augmented_manifest_id: Blake3("bd2ac888be110f12ab95a80ed850049c85f759113728271bcb98e11f00fb6bc8"),
                      augmented_manifest_size: 207,
                  },
              ),
          ),
      ],
      sapling_tree_blob_size: 89,
  }



Empty local/shared caches.
  $ setconfig remotefilelog.cachepath=$TESTTMP/cache2
  $ newclientrepo client2 server
  $ hg pull -qB main

scmstore can fetch (pure) file content from CAS:
  $ hg debugscmstore -r $A --mode file --pure-content A
  Successfully fetched file: StoreFile {
      content: Some(
          Cas(
              b"A",
          ),
      ),
      aux_data: Some(
          FileAuxData {
              total_size: 1,
              sha1: Sha1("6dcd4ce23d88e2ee9568ba546c007c63d9131c1b"),
              blake3: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"),
              file_header_metadata: Some(
                  b"",
              ),
          },
      ),
  }

Empty shared caches.
  $ setconfig remotefilelog.cachepath=$TESTTMP/cache3

scmstore can fetch trees from CAS:

First fetch root tree to trigger fetching "dir" aux data:
  $ hg debugscmstore -r $A --mode tree "" --config devel.print-metrics=scmstore.tree.fetch.aux >/dev/null
  scmstore.tree.fetch.aux.cache.misses: 1

Then fetch "dir" from CAS:
  $ hg debugscmstore -r $A --mode tree "dir" --config devel.print-metrics=scmstore.tree.fetch.aux
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("1d12614b1101689fa2c31b749d0c17fa9ad10564"),
      },
      StoreTree {
          content: Some(
              Cas(
                  AugmentedTreeWithDigest {
                      augmented_manifest_id: Blake3("bd2ac888be110f12ab95a80ed850049c85f759113728271bcb98e11f00fb6bc8"),
                      augmented_manifest_size: 207,
                      augmented_tree: AugmentedTree {
                          hg_node_id: HgId("1d12614b1101689fa2c31b749d0c17fa9ad10564"),
                          computed_hg_node_id: None,
                          p1: None,
                          p2: None,
                          entries: [
                              (
                                  PathComponentBuf(
                                      "file",
                                  ),
                                  FileNode(
                                      AugmentedFileNode {
                                          file_type: Regular,
                                          filenode: HgId("755659c90eeec95dba978cb076b6aab1a02fb313"),
                                          content_blake3: Blake3("7fdd58185ed7a18ef36521aa4f017af4ad6b79ecd080030f874db1faa20a26f6"),
                                          content_sha1: Sha1("4a756ca07e9487f482465a99e8286abc86ba4dc7"),
                                          total_size: 8,
                                          file_header_metadata: None,
                                      },
                                  ),
                              ),
                          ],
                          sapling_tree_blob_size: 46,
                      },
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )
  scmstore.tree.fetch.aux.cache.hits: 1

Empty the caches
  $ setconfig remotefilelog.cachepath=$TESTTMP/cache4

Make sure prefetch uses CAS:
  $ LOG=cas=debug,eagerepo=debug hg prefetch -r $A .
  DEBUG cas: creating eager remote client
  DEBUG cas: created client
  DEBUG cas: EagerRepoStore fetching 1 tree(s)
  DEBUG cas: EagerRepoStore fetching 1 tree(s)
  DEBUG cas: EagerRepoStore fetching 1 tree(s)
  DEBUG cas: EagerRepoStore prefetching 2 file(s)

Don't rewrite aux data to cache:
  $ LOG=revisionstore=trace hg prefetch -r $A . 2>&1 | grep "writing to"
  [1]


Make sure we don't fetch from local cache unnecessarily.
  $ hg debugscmstore -r $A --mode tree "dir" --config devel.print-metrics=scmstore.tree.fetch.indexedlog.cache.keys >/dev/null
  $ hg debugscmstore -r $A --mode file "dir/file" --config devel.print-metrics=scmstore.file.fetch.indexedlog.cache.keys >/dev/null

And sanity check the counter we are looking for exists:
  $ hg debugscmstore -r $A --mode tree "dir" --config devel.print-metrics=scmstore.tree.fetch.indexedlog.cache.keys --config scmstore.cas-mode=off >/dev/null
  scmstore.tree.fetch.indexedlog.cache.keys: 1
  $ hg debugscmstore -r $A --mode file "dir/file" --config devel.print-metrics=scmstore.file.fetch.indexedlog.cache.keys --config scmstore.cas-mode=off >/dev/null
  scmstore.file.fetch.indexedlog.cache.keys: 1

Count duplicate key fetches properly:
  $ hg dbsh -c "n = repo['$A']['dir/file'].filenode(); repo.fileslog.filestore.prefetch([('foo', n), ('bar', n)])" --config devel.print-metrics=scmstore.file.fetch.cas >/dev/null
  scmstore.file.fetch.cas.hits: 2
  scmstore.file.fetch.cas.keys: 2
  scmstore.file.fetch.cas.requests: 1
  scmstore.file.fetch.cas.time: * (glob)


Make sure fall back to non-CAS fetching works:
  $ FAILPOINTS=eagerepo::cas=return hg cat -r $A dir/file
  contents (no-eol)
