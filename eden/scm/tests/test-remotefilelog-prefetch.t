#debugruntest-compatible

  $ eagerepo

  $ newrepo server
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS

  $ newclientrepo client test:server

First, sanity that we don't have any data locally:
  $ hg debugscmstore -r $A A --local --mode=file
  abort: unknown revision '426bada5c67598ca65036d57d9e4b64b0c1ce7a0'
  [255]

  $ hg prefetch -q -r $A

Now we do have aux data locally:
  $ hg debugscmstore -r $A A --local --mode=file --config scmstore.compute-aux-data=false
  Successfully fetched file: StoreFile {
      content: Some(
          IndexedLog(
              Entry {
                  key: Key {
                      path: RepoPathBuf(
                          "A",
                      ),
                      hgid: HgId("005d992c5dcf32993668f7cede29d296c494a5d9"),
                  },
                  metadata: Metadata {
                      size: None,
                      flags: None,
                  },
                  content: OnceCell(Uninit),
                  compressed_content: Some(
                      b"\x01\x00\x00\x00\x10A",
                  ),
              },
          ),
      ),
      aux_data: Some(
          FileAuxData {
              total_size: 1,
              content_id: ContentId("eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9"),
              sha1: Sha1("6dcd4ce23d88e2ee9568ba546c007c63d9131c1b"),
              sha256: Sha256("559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd"),
              seeded_blake3: Some(
                  Blake3("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"),
              ),
          },
      ),
  }
