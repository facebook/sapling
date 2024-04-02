#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ enable crdump remotenames

  $ showgraph() {
  >    hg log -G -T "{desc}: {phase} {bookmarks} {remotenames}" -r "all()"
  > }

Setup server

  $ hg init server
  $ cd server
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
  pulling from test:server
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
  $ hg debugcrdump -r . | jq '.commits[].branch'
  "bookmark1"
#endif
