
#require no-eden


  $ eagerepo

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
                      tree_aux_data: None,
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )

  $ setconfig remotefilelog.cachepath=$TESTTMP/cache2

Fetch a tree with children metadata:
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
                      tree_aux_data: None,
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
                      tree_aux_data: None,
                  },
              ),
          ),
          parents: None,
          aux_data: None,
      },
  )
