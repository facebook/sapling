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

FIXME Now we do have aux data locally:
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
      aux_data: None,
  }
