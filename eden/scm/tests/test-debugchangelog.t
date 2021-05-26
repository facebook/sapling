#chg-compatible

  $ configure modern
  $ setconfig treemanifest.flatcompat=0

  $ newrepo
  $ drawdag << 'EOS'
  > B C
  > |/|
  > A D
  > | |
  > E F
  > | |
  > G H
  > EOS

  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (revlog):
    Local:
      Revlog: $TESTTMP/repo1/.hg/store/00changelog.{i,d}
      Nodemap: $TESTTMP/repo1/.hg/store/00changelog.nodemap
  Feature Providers:
    Commit Graph Algorithms:
      Revlog
    Commit Hash / Rev Lookup:
      Nodemap
    Commit Data (user, message):
      Revlog

  $ hg debugchangelog --config experimental.rust-commits=0
  The changelog is backed by Python + C revlog.

  $ hg debugchangelog --config experimental.rust-commits=0 --config extensions.clindex=
  The changelog is backed by Python + C revlog.
  The clindex extension is used for commit hash lookups.

  $ hg log -Gr 'all()' -T '{desc}'
  o    C
  ├─╮
  │ │ o  B
  ├───╯
  │ o  D
  │ │
  o │  A
  │ │
  │ o  F
  │ │
  o │  E
  │ │
  │ o  H
  │
  o  G
  

Migration
=========

  $ hg debugchangelog --migrate foobar
  abort: invalid changelog format: foobar
  [255]

To Python revlog:

  $ hg debugchangelog --migrate pythonrevlog
  $ hg debugchangelog
  The changelog is backed by Python + C revlog.

To Rust revlog:

  $ hg debugchangelog --migrate rustrevlog
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (revlog):
    Local:
      Revlog: $TESTTMP/repo1/.hg/store/00changelog.{i,d}
      Nodemap: $TESTTMP/repo1/.hg/store/00changelog.nodemap
  Feature Providers:
    Commit Graph Algorithms:
      Revlog
    Commit Hash / Rev Lookup:
      Nodemap
    Commit Data (user, message):
      Revlog

To doublewrite:

  $ hg debugchangelog --migrate doublewrite
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (doublewrite):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Zstore: $TESTTMP/repo1/.hg/store/hgcommits/v1
      Revlog + Nodemap: $TESTTMP/repo1/.hg/store/00changelog.{i,d,nodemap}
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Zstore (incomplete)
      Revlog
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  │ o  C
  ╭─┤
  │ o  D
  │ │
  │ o  F
  │ │
  │ o  H
  │
  o  A
  │
  o  E
  │
  o  G
  

To full segments:

  $ hg debugchangelog --migrate fullsegments
  $ hg debugchangelog --debug
  The changelog is backed by Rust. More backend information:
  Backend (non-lazy segments):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Zstore: $TESTTMP/repo1/.hg/store/hgcommits/v1
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Zstore
  Max Level: 1
   Level 1
    Group Master:
     Next Free Id: 0
     Segments: 0
    Group Non-Master:
     Next Free Id: N8
     Segments: 1
      1fc8102cda62+N0 : 5e98a0f69ae0+N6 [] Root
   Level 0
    Group Master:
     Next Free Id: 0
     Segments: 0
    Group Non-Master:
     Next Free Id: N8
     Segments: 4
      f535a6a0548e+N7 : f535a6a0548e+N7 [4ec7ca77ac1a+N2]
      5e98a0f69ae0+N6 : 5e98a0f69ae0+N6 [4ec7ca77ac1a+N2, 50e53efd5222+N5]
      e7050b6e5048+N3 : 50e53efd5222+N5 [] Root
      1fc8102cda62+N0 : 4ec7ca77ac1a+N2 [] Root

The segments backend does not need revlog data.

  $ rm -rf .hg/store/00changelog*
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  │ o  C
  ╭─┤
  │ o  D
  │ │
  │ o  F
  │ │
  │ o  H
  │
  o  A
  │
  o  E
  │
  o  G
  

To revlog:

  $ hg debugchangelog --migrate revlog
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (revlog):
    Local:
      Revlog: $TESTTMP/repo1/.hg/store/00changelog.{i,d}
      Nodemap: $TESTTMP/repo1/.hg/store/00changelog.nodemap
  Feature Providers:
    Commit Graph Algorithms:
      Revlog
    Commit Hash / Rev Lookup:
      Nodemap
    Commit Data (user, message):
      Revlog

The revlog backend does not need segmented data.

  $ rm -rf .hg/store/segments .hg/store/hgcommits
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  │ o  C
  ╭─┤
  │ o  D
  │ │
  │ o  F
  │ │
  │ o  H
  │
  o  A
  │
  o  E
  │
  o  G
  
To doublewrite:

  $ hg debugchangelog --migrate lazytext
  abort: lazytext backend can only be migrated from hybrid or doublewrite
  [255]

  $ hg debugchangelog --migrate lazytext --unless doublewrite --unless revlog

  $ hg debugchangelog --migrate doublewrite

Prepare the "master" group. Note the "Group Master" output in debugchangelog:

  $ setconfig paths.default=test:server1
  $ hg push -q -r 'desc(C)' --to master --create
  $ hg push -q -r 'desc(B)' --allow-anon
  $ hg pull -q -B master

  $ hg debugchangelog --debug
  The changelog is backed by Rust. More backend information:
  Backend (doublewrite):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Zstore: $TESTTMP/repo1/.hg/store/hgcommits/v1
      Revlog + Nodemap: $TESTTMP/repo1/.hg/store/00changelog.{i,d,nodemap}
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Zstore (incomplete)
      Revlog
  Max Level: 0
   Level 0
    Group Master:
     Next Free Id: 7
     Segments: 3
      5e98a0f69ae0+6 : 5e98a0f69ae0+6 [4ec7ca77ac1a+2, 50e53efd5222+5] OnlyHead
      e7050b6e5048+3 : 50e53efd5222+5 [] Root
      1fc8102cda62+0 : 4ec7ca77ac1a+2 [] Root OnlyHead
    Group Non-Master:
     Next Free Id: N1
     Segments: 1
      f535a6a0548e+N0 : f535a6a0548e+N0 [4ec7ca77ac1a+2]

To lazy:

  $ hg debugchangelog --migrate lazytext

  $ hg debugchangelog --migrate lazy

  $ hg debugchangelog --migrate lazy

  $ hg debugchangelog --migrate doublewrite --unless lazy

  $ LOG=dag::protocol=debug hg log -Gr 'all()' -T '{desc} {remotenames}'
  o  B
  │
  │ o  C remote/master
  ╭─┤
  │ o  D
  │ │
  │ o  F
  │ │
   DEBUG dag::protocol: resolve names [0000000000000000000000000000000000000000] remotely
  │ o  H
  │
  o  A
  │
  o  E
  │
  o  G
  
