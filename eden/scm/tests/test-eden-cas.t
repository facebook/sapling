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
  $ hg ci -Aqm C 2>&1 | sed '/Error/q'
  transaction abort!
  rollback completed
  abort: EdenService::resetParentCommits failed with ApplicationException
  
  Caused by:
      Unknown: rust::cxxbridge1::Error: Network Error: server responded 404 Not Found for eager://$TESTTMP/server/trees: eadd25abb6eb44211634adf30647c754a76c10f6 cannot be found. Headers: {}
