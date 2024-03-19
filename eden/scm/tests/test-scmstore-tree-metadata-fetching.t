#debugruntest-compatible

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

Fetch a tree:
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

FIXME We should also have aux data for the files available:
  $ hg debugscmstore -r $A dir/file1 --mode=file --local
  Failed to fetch file: Key {
      path: RepoPathBuf(
          "dir/file1",
      ),
      hgid: HgId("a58629e4c3c5a5d14b5810b2e35681bb84319167"),
  }
  Error: [not found locally and not contacting server]
