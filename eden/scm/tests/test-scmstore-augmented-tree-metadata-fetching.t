
#require no-eden


  $ eagerepo
  $ setconfig scmstore.fetch-tree-aux-data=true
  $ setconfig scmstore.store-tree-aux-data=true

  $ newrepo server
  $ drawdag <<EOS
  > A  # A/dir/file1=file1
  >    # A/dir/file2=file2
  >    # A/dir/dir/file3=file3
  >    # A/file=file
  > EOS

  $ newclientrepo client test:server
  $ hg pull -q -r $A

Sanity check that children metadata isn't fetched by default:
  $ hg debugscmstore -r $A dir --mode=tree
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: Some(
              SaplingRemoteApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\x00ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\x00a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\x00ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                      parents: Some(
                          None,
                      ),
                      children: None,
                      tree_aux_data: Some(
                          DirectoryMetadata {
                              augmented_manifest_id: Blake3("3db383bed414336a1d6673620506fa927a6c53f9052390487f11821b2547b585"),
                              augmented_manifest_size: 481,
                          },
                      ),
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )

  $ setconfig remotefilelog.cachepath=$TESTTMP/cache2

Fetch a tree with children metadata, make sure directories aux data also returned:
  $ hg debugscmstore -r $A dir --mode=tree --config scmstore.tree-metadata-mode=always
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: Some(
              SaplingRemoteApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\x00ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\x00a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\x00ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                      parents: Some(
                          None,
                      ),
                      children: Some(
                          [
                              Ok(
                                  Directory(
                                      TreeChildDirectoryEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "dir",
                                              ),
                                              hgid: HgId("ac934ed5f01e06c92b6c95661b2ccaf2a734509f"),
                                          },
                                          tree_aux_data: Some(
                                              DirectoryMetadata {
                                                  augmented_manifest_id: Blake3("16f534257ecef0fc9254628292cd025db76e73e1c013419fca0b7f02f9fb91c6"),
                                                  augmented_manifest_size: 208,
                                              },
                                          ),
                                      },
                                  ),
                              ),
                              Ok(
                                  File(
                                      TreeChildFileEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "file1",
                                              ),
                                              hgid: HgId("a58629e4c3c5a5d14b5810b2e35681bb84319167"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  content_id: ContentId("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  size: 5,
                                                  content_sha1: Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
                                                  content_sha256: Sha256("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  content_blake3: Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
                                                  file_header_metadata: Some(
                                                      b"",
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                              Ok(
                                  File(
                                      TreeChildFileEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "file2",
                                              ),
                                              hgid: HgId("ecbe8b3047eb5d9bb298f516d451f64491812e07"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  content_id: ContentId("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  size: 5,
                                                  content_sha1: Sha1("cb99b709a1978bd205ab9dfd4c5aaa1fc91c7523"),
                                                  content_sha256: Sha256("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  content_blake3: Blake3("aab0b64d0a516f16e06cd7571dece3e6cc6f57ca2462ce69872d3d7e6664e7da"),
                                                  file_header_metadata: Some(
                                                      b"",
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                          ],
                      ),
                      tree_aux_data: Some(
                          DirectoryMetadata {
                              augmented_manifest_id: Blake3("3db383bed414336a1d6673620506fa927a6c53f9052390487f11821b2547b585"),
                              augmented_manifest_size: 481,
                          },
                      ),
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )

We should also have aux data for the files available as a side effect of tree fetching:
  $ hg debugscmstore -r $A dir/file1 --mode=file --fetch-mode=LOCAL
  Successfully fetched file: StoreFile {
      content: None,
      aux_data: Some(
          FileAuxData {
              total_size: 5,
              sha1: Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
              blake3: Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
              file_header_metadata: Some(
                  b"",
              ),
          },
      ),
  }


  $ setconfig remotefilelog.cachepath=$TESTTMP/cache3

Fetch mode can also trigger tree metadata fetch:

  $ hg debugscmstore -r $A dir --mode=tree --fetch-mode='LOCAL|REMOTE|PREFETCH'
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: Some(
              SaplingRemoteApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\x00ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\x00a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\x00ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                      parents: Some(
                          None,
                      ),
                      children: Some(
                          [
                              Ok(
                                  Directory(
                                      TreeChildDirectoryEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "dir",
                                              ),
                                              hgid: HgId("ac934ed5f01e06c92b6c95661b2ccaf2a734509f"),
                                          },
                                          tree_aux_data: Some(
                                              DirectoryMetadata {
                                                  augmented_manifest_id: Blake3("16f534257ecef0fc9254628292cd025db76e73e1c013419fca0b7f02f9fb91c6"),
                                                  augmented_manifest_size: 208,
                                              },
                                          ),
                                      },
                                  ),
                              ),
                              Ok(
                                  File(
                                      TreeChildFileEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "file1",
                                              ),
                                              hgid: HgId("a58629e4c3c5a5d14b5810b2e35681bb84319167"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  content_id: ContentId("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  size: 5,
                                                  content_sha1: Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
                                                  content_sha256: Sha256("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  content_blake3: Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
                                                  file_header_metadata: Some(
                                                      b"",
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                              Ok(
                                  File(
                                      TreeChildFileEntry {
                                          key: Key {
                                              path: RepoPathBuf(
                                                  "file2",
                                              ),
                                              hgid: HgId("ecbe8b3047eb5d9bb298f516d451f64491812e07"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  content_id: ContentId("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  size: 5,
                                                  content_sha1: Sha1("cb99b709a1978bd205ab9dfd4c5aaa1fc91c7523"),
                                                  content_sha256: Sha256("0000000000000000000000000000000000000000000000000000000000000000"),
                                                  content_blake3: Blake3("aab0b64d0a516f16e06cd7571dece3e6cc6f57ca2462ce69872d3d7e6664e7da"),
                                                  file_header_metadata: Some(
                                                      b"",
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                          ],
                      ),
                      tree_aux_data: Some(
                          DirectoryMetadata {
                              augmented_manifest_id: Blake3("3db383bed414336a1d6673620506fa927a6c53f9052390487f11821b2547b585"),
                              augmented_manifest_size: 481,
                          },
                      ),
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )


  $ newclientrepo client2 test:server
  $ hg pull -q -r $A
  $ setconfig remotefilelog.cachepath=$TESTTMP/cache4

Show we can fetch tree aux data even if plain tree is already available locally.

First fetch plain tree:
  $ hg debugscmstore -r $A dir --mode=tree --config scmstore.fetch-tree-aux-data=false
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: Some(
              SaplingRemoteApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\x00ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\x00a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\x00ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                      parents: Some(
                          None,
                      ),
                      children: None,
                      tree_aux_data: None,
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )

Verify we do have tree locally, but don't have aux data locally:
  $ hg debugscmstore -r $A dir --mode=tree --fetch-mode=LOCAL
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: Some(
              IndexedLog(
                  Entry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      metadata: Metadata {
                          size: None,
                          flags: None,
                      },
                      content: OnceCell(Uninit),
                      compressed_content: Some(
                          b"\x8c\x00\x00\x00\xf1Mdir\x00ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\x00a58629e4c3c5a5d14b5810b2e35681bb84319167/\x00\xf0\x1c2\x00ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )

  $ hg debugscmstore -r $A dir --mode=tree --aux-only --fetch-mode=LOCAL
  Failed to fetch tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      "not found locally and not contacting server",
  )

Can fetch remotely:

  $ LOG=eagerepo=debug hg debugscmstore -r $A dir --mode=tree --aux-only
  DEBUG eagerepo::api: trees * (glob)
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: None,
          parents: None,
          aux_data: Some(
              DirectoryMetadata {
                  augmented_manifest_id: Blake3("3db383bed414336a1d6673620506fa927a6c53f9052390487f11821b2547b585"),
                  augmented_manifest_size: 481,
              },
          ),
      },
  )

Make sure repeat query doesn't trigger another edenapi fetch:

  $ LOG=eagerepo=debug hg debugscmstore -r $A dir --mode=tree --aux-only
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
      },
      StoreTree {
          content: None,
          parents: None,
          aux_data: Some(
              DirectoryMetadata {
                  augmented_manifest_id: Blake3("3db383bed414336a1d6673620506fa927a6c53f9052390487f11821b2547b585"),
                  augmented_manifest_size: 481,
              },
          ),
      },
  )
