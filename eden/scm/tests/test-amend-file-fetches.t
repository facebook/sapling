#require no-eden

  $ enable amend morestatus rebase
  $ setconfig drawdag.defaultfiles=false

Make sure we minimize content fetches:
  $ newserver server
  $ drawdag <<EOS
  > B    # A/c_dir/c = c
  > |    # A/b_dir/b = b
  > A    # A/a_dir/a = a
  >      # A/.hgdirsync = foo.bar = baz\n
  > EOS

  $ newclientrepo client test:server
  $ enable dirsync
  $ hg go -q $B
  $ hg rm a_dir/a
  $ echo b >> b_dir/b
  $ echo c >> c_dir/c
  $ echo d > d
  $ hg add d
  $ LOG=file_fetches=trace,tree_fetches=trace hg amend -q
  TRACE tree_fetches: attrs=["content"] keys=["@eaa109a0"]
  TRACE file_fetches: attrs=["content"] keys=[".hgdirsync"]
  TRACE tree_fetches: attrs=["content"] keys=["a_dir@4f20beec", "b_dir@79a5124d", "c_dir@bc250767"]
  TRACE file_fetches: attrs=["content", "header", "aux"] keys=["b_dir/b", "c_dir/c"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["b_dir/b", "c_dir/c"]
  TRACE file_fetches: attrs=["content", "header"] keys=["b_dir/b"]
  TRACE file_fetches: attrs=["content", "header"] keys=["c_dir/c"]
  TRACE file_fetches: attrs=["content", "header"] keys=[".hgdirsync"]
