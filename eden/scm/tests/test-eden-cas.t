"eden restart" doesn't seem to work on Windows
#require eden no-windows

  $ setconfig scmstore.fetch-from-cas=true scmstore.fetch-tree-aux-data=true scmstore.tree-metadata-mode=always

  $ eden restart &>/dev/null

  $ newserver server
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS

  $ newclientrepo client test:server
  $ hg go -q $B
  $ cat B
  B (no-eol)
  $ echo C > C
  $ hg ci -Aqm C
