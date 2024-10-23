#require no-eden

  $ setconfig scmstore.fetch-from-cas=true scmstore.fetch-tree-aux-data=true scmstore.tree-metadata-mode=always

  $ enable rebase

  $ newserver server
  $ drawdag <<EOS
  > B  # B/renamed = foo\n (renamed from foo)
  > |
  > A  # A/foo = foo\n
  > EOS

  $ newclientrepo client test:server
  $ hg go -q $B
FIXME: should be able to fetch hg content (with header)
  $ hg dbsh -c "repo['$B']['renamed'].data()" 2>&1 | grep UncategorizedNativeError
  error.UncategorizedNativeError: CAS data has no copy info
