#require no-eden

  $ enable amend morestatus rebase
  $ setconfig drawdag.defaultfiles=false

Make sure we minimize content fetches:
  $ newserver server
  $ drawdag <<EOS
  > B    # A/c_dir/c = c
  > |    # A/b_dir/b = b
  > A    # A/a_dir/a = a
  > EOS

  $ newclientrepo client test:server
  $ hg go -q $B
  $ hg rm a_dir/a
  $ echo b >> b_dir/b
  $ echo c >> c_dir/c
  $ echo d > d
  $ hg add d
  $ LOG=file_fetches=trace,tree_fetches=trace hg amend -q
  TRACE tree_fetches: attrs=["content"] keys=["@3a57afd5"]
  TRACE tree_fetches: attrs=["content"] keys=["a_dir@4f20beec"]
  TRACE tree_fetches: attrs=["content"] keys=["b_dir@79a5124d"]
  TRACE tree_fetches: attrs=["content"] keys=["c_dir@bc250767"]
  TRACE file_fetches: attrs=["content", "header"] keys=["b_dir/b"]
  TRACE file_fetches: attrs=["content", "header"] keys=["c_dir/c"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["b_dir/b"]
  TRACE file_fetches: attrs=["history"] length=Some(1) keys=["c_dir/c"]
