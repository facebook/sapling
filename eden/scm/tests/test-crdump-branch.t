#debugruntest-compatible

  $ configure modern
  $ enable crdump remotenames

  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

Setup server

  $ newserver server
  $ cd $TESTTMP/server
  $ drawdag <<EOS
  > Y
  > |
  > X
  > EOS
  $ hg bookmark -r $X bookmark1
  $ hg bookmark -r $X bookmark1.1
  $ hg bookmark -r $Y bookmark2

Setup client

  $ cd $TESTTMP
  $ clone server client
  $ cd client
  $ hg pull -B bookmark1 -B bookmark2 -B bookmark1.1
  pulling from ssh://user@dummy/server
  $ hg goto -r bookmark1 -q
  $ echo 1 >> a
  $ hg ci -Am a
  adding a

  $ showgraph
  @  a: draft
  │
  │ o  Y: public  remote/bookmark2
  ├─╯
  o  X: public  remote/bookmark1 remote/bookmark1.1

#if jq
  # fixme
  $ hg debugcrdump -r . | jq '.commits[].branch'
  ""
#endif
