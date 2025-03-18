"eden restart" doesn't seem to work on Windows
#require eden no-windows

  $ setconfig scmstore.cas-mode=on

  $ eden restart &>/dev/null

  $ newserver server
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS

  $ newclientrepo client server
  $ hg go -q $B
  $ cat B
  B (no-eol)
  $ echo C > C
  $ hg ci -Aqm C
