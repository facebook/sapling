#require no-eden

  $ setconfig scmstore.fetch-from-cas=true scmstore.fetch-tree-aux-data=true

  $ enable rebase

  $ newserver server
  $ drawdag <<EOS
  > B  # B/renamed = foo\n (renamed from foo)
  > |
  > A  # A/foo = foo\n
  > EOS

  $ newclientrepo client test:server
  $ hg go -q $B
  $ hg dbsh -c "print(repo['$B']['renamed'].data())"
  b'foo\n'
  $ hg dbsh -c "r = repo['$B']['renamed'].renamed(); print((r[0], hex(r[1])))"
  ('foo', '2ed2a3912a0b24502043eae84ee4b279c18b90dd')
