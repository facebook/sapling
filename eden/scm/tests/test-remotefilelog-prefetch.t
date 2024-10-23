#require no-eden

  $ eagerepo

  $ newrepo server
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS

  $ newclientrepo client test:server

First, sanity that we don't have any data locally:
  $ hg debugscmstore -r $A A --fetch-mode=LOCAL --mode=file
  abort: unknown revision '426bada5c67598ca65036d57d9e4b64b0c1ce7a0'
  [255]

Prefetch (and also check we get counters):
  $ hg prefetch -q -r $A --config devel.print-metrics=scmstore
  scmstore.file.api.hg_prefetch.calls: 1
  scmstore.file.api.hg_prefetch.keys: 1
  scmstore.file.api.hg_prefetch.singles: 1
  scmstore.file.api.hg_refresh.calls: 3
  scmstore.file.fetch.aux.cache.keys: 1
  scmstore.file.fetch.aux.cache.misses: 1
  scmstore.file.fetch.aux.cache.requests: 1
  scmstore.file.fetch.aux.cache.time: * (glob) (?)
  scmstore.file.fetch.edenapi.hits: 1
  scmstore.file.fetch.edenapi.keys: 1
  scmstore.file.fetch.edenapi.requests: 1
  scmstore.file.fetch.edenapi.time: * (glob) (?)
  scmstore.file.fetch.indexedlog.cache.keys: 1
  scmstore.file.fetch.indexedlog.cache.misses: 1
  scmstore.file.fetch.indexedlog.cache.requests: 1
  scmstore.file.fetch.indexedlog.cache.time: * (glob) (?)
  scmstore.file.fetch.indexedlog.local.keys: 1
  scmstore.file.fetch.indexedlog.local.misses: 1
  scmstore.file.fetch.indexedlog.local.requests: 1
  scmstore.file.fetch.indexedlog.local.time: * (glob) (?)
  scmstore.tree.fetch.edenapi.keys: 1
  scmstore.tree.fetch.edenapi.requests: 1
  scmstore.tree.fetch.edenapi.time: * (glob) (?)
  scmstore.tree.fetch.indexedlog.cache.keys: 1
  scmstore.tree.fetch.indexedlog.cache.misses: 1
  scmstore.tree.fetch.indexedlog.cache.requests: 1
  scmstore.tree.fetch.indexedlog.cache.time: * (glob) (?)
  scmstore.tree.fetch.indexedlog.local.keys: 1
  scmstore.tree.fetch.indexedlog.local.misses: 1
  scmstore.tree.fetch.indexedlog.local.requests: 1
  scmstore.tree.fetch.indexedlog.local.time: * (glob) (?)

Now we do have aux data locally:
  $ hg debugscmstore -r $A A --fetch-mode=LOCAL --mode=file
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
              Hg,
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


Fetch only content first:
  $ hg cat -q -r $B B
  B (no-eol)

Make sure we don't have aux data yet:
  $ hg debugscmstore -r $B B --fetch-mode=LOCAL --mode=file --config scmstore.compute-aux-data=false
  Successfully fetched file: StoreFile {
      content: Some(
          IndexedLog(
              Entry {
                  key: Key {
                      path: RepoPathBuf(
                          "B",
                      ),
                      hgid: HgId("35e7525ce3a48913275d7061dd9a867ffef1e34d"),
                  },
                  metadata: Metadata {
                      size: None,
                      flags: None,
                  },
                  content: OnceCell(Uninit),
                  compressed_content: Some(
                      b"\x01\x00\x00\x00\x10B",
                  ),
              },
              Hg,
          ),
      ),
      aux_data: None,
  }

Fetching only aux data does not trigger a remote query:
  $ LOG=eagerepo::api=debug hg debugscmstore -r $B B --aux-only --mode=file --config devel.print-metrics=scmstore.file.fetch.aux
  Successfully fetched file: StoreFile {
      content: None,
      aux_data: Some(
          FileAuxData {
              total_size: 1,
              sha1: Sha1("ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec"),
              blake3: Blake3("5667f2421ac250c4bb9af657b5ead3cdbd940bfbc350b2bfee47454643832b48"),
              file_header_metadata: None,
          },
      ),
  }
  scmstore.file.fetch.aux.cache.computed: 1
  scmstore.file.fetch.aux.cache.keys: 1
  scmstore.file.fetch.aux.cache.misses: 1
  scmstore.file.fetch.aux.cache.requests: 1
  scmstore.file.fetch.aux.cache.time: * (glob) (?)
