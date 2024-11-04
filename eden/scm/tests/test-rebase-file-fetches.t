  $ enable amend morestatus rebase
  $ setconfig rebase.experimental.inmemory=true
  $ setconfig drawdag.defaultfiles=false

  $ newserver server
  $ drawdag <<EOS
  >      # C/four = four
  >      # B/two = 2
  > C B  # B/three = three
  > |/   # B/one = (removed)
  > A    # A/one = one
  >      # A/two = two
  > EOS

  $ newclientrepo client test:server
  $ LOG=file_fetches=trace,tree_fetches=trace hg rebase -q -r $B -d $C
  TRACE tree_fetches: attrs=["content"] keys=["@0d27acda"]
  TRACE tree_fetches: attrs=["content"] keys=["@e6f8ae7d"]
  TRACE tree_fetches: attrs=["content"] keys=["@b941fe6c"]
  TRACE file_fetches: attrs=["header"] keys=["three"]
  TRACE file_fetches: attrs=["header"] keys=["three"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["three", "two"]

  $ newserver server2
  $ drawdag <<EOS
  >   C  # C/a/b/c2/file = C
  >   |
  > D B  # D/a/b/c3/file = D
  > |/   # B/a/b/c1/file = B
  > A    # A/a/b/c2/file = A
  >      # A/a/b/c1/file = A
  > EOS

  $ newclientrepo client2 test:server2
  $ hg pull -qr $C
  $ LOG=file_fetches=trace,tree_fetches=trace hg rebase -q -s $B -d $D
  TRACE tree_fetches: attrs=["content"] keys=["@0578004a"]
  TRACE tree_fetches: attrs=["content"] keys=["@3b9f2e11"]
  TRACE tree_fetches: attrs=["content"] keys=["@93377924"]
  TRACE tree_fetches: attrs=["content"] keys=["a@1da49c91", "a@82fb1620"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b@693cd354", "a/b@ee58f75d"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b/c1@0c8dfc95", "a/b/c1@82bbf75d"]
  TRACE tree_fetches: attrs=["content"] keys=["a@e8e8d5ec"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b@804141b9"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["a/b/c1/file"]
  TRACE tree_fetches: attrs=["content"] keys=["@e2120c7c"]
  TRACE tree_fetches: attrs=["content"] keys=["@7dd79201"]
  TRACE tree_fetches: attrs=["content"] keys=["a@05099e49"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b@99574908"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b/c2@e98395d2"]
  TRACE tree_fetches: attrs=["content"] keys=["a@348e2e56"]
  TRACE tree_fetches: attrs=["content"] keys=["a/b@105dfd91"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["a/b/c2/file"]
