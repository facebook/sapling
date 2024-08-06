  $ eagerepo

  $ newrepo server
  $ drawdag <<EOS
  > B  # B/dir/file=bar
  > |
  > A  # A/dir/file=foo
  > EOS

  $ newclientrepo client test:server
  $ hg pull -qr $B

We can fetch tree parents:
  $ hg debugscmstore -r $B dir --mode=tree --tree-parents
  Successfully fetched tree: (
      Key {
          path: RepoPathBuf(
              "dir",
          ),
          hgid: HgId("79bcde5aa72548a14e67f99c571fac4552005120"),
      },
      StoreTree {
          content: Some(
              SaplingRemoteApi(
                  TreeEntry {
                      key: Key {
                          path: RepoPathBuf(
                              "dir",
                          ),
                          hgid: HgId("79bcde5aa72548a14e67f99c571fac4552005120"),
                      },
                      data: Some(
                          b"file\x001135da73f7677c360ac31ecde4ee16e47f0529f5\n",
                      ),
                      parents: Some(
                          One(
                              HgId("bef2f8ab81cdaf48e171ab7793b536eeeea649b8"),
                          ),
                      ),
                      children: None,
                      tree_aux_data: None,
                  },
              ),
          ),
          parents: Some(
              One(
                  HgId("bef2f8ab81cdaf48e171ab7793b536eeeea649b8"),
              ),
          ),
          aux_data: None,
      },
  )
