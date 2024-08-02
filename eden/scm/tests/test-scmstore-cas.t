  $ setconfig push.edenapi=true

First sanity check eagerepo works as CAS store
  $ newclientrepo
  $ echo "A" | drawdag

Remote doesn't know about commit yet.
  $ hg debugcas -r $A A
  file path A, node 005d992c5dcf32993668f7cede29d296c494a5d9, digest CasDigest { hash: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"), size: 1 }, not found in CAS

  $ hg push -qr $A --to main --create

Now remote knows about our data.
  $ hg debugcas -r $A A
  file path A, node 005d992c5dcf32993668f7cede29d296c494a5d9, digest CasDigest { hash: Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"), size: 1 }, contents:
  A

Can also fetch tree data.
  $ hg debugcas -r $A ""
  tree path , node 41b34f08c1356f6ad068e9ab9b43d984245111aa, digest CasDigest { hash: Blake3("f0aef0c3978f2947b763a1f87ff5c68611125192cca9d0e95cb18787740eae3b"), size: 204 }, contents:
  AugmentedTree {
      hg_node_id: HgId("41b34f08c1356f6ad068e9ab9b43d984245111aa"),
      computed_hg_node_id: None,
      p1: None,
      p2: None,
      entries: [
          (
              RepoPathBuf(
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
      ],
  }

