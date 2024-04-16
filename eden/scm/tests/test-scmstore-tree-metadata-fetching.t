#debugruntest-compatible

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
              EdenApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\0ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\0a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\0ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
                      ),
                      parents: Some(
                          None,
                      ),
                      children: None,
                  },
              ),
          ),
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
              EdenApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\0ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\0a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\0ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
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
                                                  "dir/file1",
                                              ),
                                              hgid: HgId("a58629e4c3c5a5d14b5810b2e35681bb84319167"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  revisionstore_flags: None,
                                                  content_id: Some(
                                                      ContentId("e814695438c861a0def69866f1d28b57827961b6dfc31c66e6ba16c517eeb9e0"),
                                                  ),
                                                  file_type: Some(
                                                      Regular,
                                                  ),
                                                  size: Some(
                                                      5,
                                                  ),
                                                  content_sha1: Some(
                                                      Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
                                                  ),
                                                  content_sha256: Some(
                                                      Sha256("c147efcfc2d7ea666a9e4f5187b115c90903f0fc896a56df9a6ef5d8f3fc9f31"),
                                                  ),
                                                  content_seeded_blake3: Some(
                                                      Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
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
                                                  "dir/file2",
                                              ),
                                              hgid: HgId("ecbe8b3047eb5d9bb298f516d451f64491812e07"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  revisionstore_flags: None,
                                                  content_id: Some(
                                                      ContentId("233fc5ebc2502409036b103a972af95424dfd522d9e41089125c7925432b11f9"),
                                                  ),
                                                  file_type: Some(
                                                      Regular,
                                                  ),
                                                  size: Some(
                                                      5,
                                                  ),
                                                  content_sha1: Some(
                                                      Sha1("cb99b709a1978bd205ab9dfd4c5aaa1fc91c7523"),
                                                  ),
                                                  content_sha256: Some(
                                                      Sha256("3377870dfeaaa7adf79a374d2702a3fdb13e5e5ea0dd8aa95a802ad39044a92f"),
                                                  ),
                                                  content_seeded_blake3: Some(
                                                      Blake3("aab0b64d0a516f16e06cd7571dece3e6cc6f57ca2462ce69872d3d7e6664e7da"),
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                          ],
                      ),
                  },
              ),
          ),
      },
  )

We should also have aux data for the files available as a side effect of tree fetching:
  $ hg debugscmstore -r $A dir/file1 --mode=file --fetch-mode=LOCAL
  Successfully fetched file: StoreFile {
      content: None,
      aux_data: Some(
          FileAuxData {
              total_size: 5,
              content_id: ContentId("e814695438c861a0def69866f1d28b57827961b6dfc31c66e6ba16c517eeb9e0"),
              sha1: Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
              sha256: Sha256("c147efcfc2d7ea666a9e4f5187b115c90903f0fc896a56df9a6ef5d8f3fc9f31"),
              seeded_blake3: Some(
                  Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
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
              EdenApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("2aabbe46539594a3aede2a262ebfbcd3107ad10c"),
                      },
                      data: Some(
                          b"dir\0ac934ed5f01e06c92b6c95661b2ccaf2a734509ft\nfile1\0a58629e4c3c5a5d14b5810b2e35681bb84319167\nfile2\0ecbe8b3047eb5d9bb298f516d451f64491812e07\n",
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
                                                  "dir/file1",
                                              ),
                                              hgid: HgId("a58629e4c3c5a5d14b5810b2e35681bb84319167"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  revisionstore_flags: None,
                                                  content_id: Some(
                                                      ContentId("e814695438c861a0def69866f1d28b57827961b6dfc31c66e6ba16c517eeb9e0"),
                                                  ),
                                                  file_type: Some(
                                                      Regular,
                                                  ),
                                                  size: Some(
                                                      5,
                                                  ),
                                                  content_sha1: Some(
                                                      Sha1("60b27f004e454aca81b0480209cce5081ec52390"),
                                                  ),
                                                  content_sha256: Some(
                                                      Sha256("c147efcfc2d7ea666a9e4f5187b115c90903f0fc896a56df9a6ef5d8f3fc9f31"),
                                                  ),
                                                  content_seeded_blake3: Some(
                                                      Blake3("0a370c8c0d1deeea00890dfa7b6c52a863d45d95ab472fae5510e4aacf674fd4"),
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
                                                  "dir/file2",
                                              ),
                                              hgid: HgId("ecbe8b3047eb5d9bb298f516d451f64491812e07"),
                                          },
                                          file_metadata: Some(
                                              FileMetadata {
                                                  revisionstore_flags: None,
                                                  content_id: Some(
                                                      ContentId("233fc5ebc2502409036b103a972af95424dfd522d9e41089125c7925432b11f9"),
                                                  ),
                                                  file_type: Some(
                                                      Regular,
                                                  ),
                                                  size: Some(
                                                      5,
                                                  ),
                                                  content_sha1: Some(
                                                      Sha1("cb99b709a1978bd205ab9dfd4c5aaa1fc91c7523"),
                                                  ),
                                                  content_sha256: Some(
                                                      Sha256("3377870dfeaaa7adf79a374d2702a3fdb13e5e5ea0dd8aa95a802ad39044a92f"),
                                                  ),
                                                  content_seeded_blake3: Some(
                                                      Blake3("aab0b64d0a516f16e06cd7571dece3e6cc6f57ca2462ce69872d3d7e6664e7da"),
                                                  ),
                                              },
                                          ),
                                      },
                                  ),
                              ),
                          ],
                      ),
                  },
              ),
          ),
      },
  )
