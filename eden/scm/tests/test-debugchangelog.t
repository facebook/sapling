#chg-compatible

  $ configure modern

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
  
  $ cp -R . $TESTTMP/revlogrepo
  $ cp -R . $TESTTMP/revlogrepo2

Migration
=========

  $ hg debugchangelog --migrate foobar
  abort: invalid changelog format: foobar
  [255]

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
     Segments: 0
    Group Non-Master:
     Segments: 1
      1fc8102cda62+N0 : 5e98a0f69ae0+N6 [] Root
   Level 0
    Group Master:
     Segments: 0
    Group Non-Master:
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
     Segments: 3
      5e98a0f69ae0+6 : 5e98a0f69ae0+6 [4ec7ca77ac1a+2, 50e53efd5222+5] OnlyHead
      e7050b6e5048+3 : 50e53efd5222+5 [] Root
      1fc8102cda62+0 : 4ec7ca77ac1a+2 [] Root OnlyHead
    Group Non-Master:
     Segments: 1
      f535a6a0548e+N0 : f535a6a0548e+N0 [4ec7ca77ac1a+2]

To lazy:

  $ hg debugchangelog --migrate lazytext

  $ hg debugchangelog --migrate lazy

  $ hg debugchangelog --migrate lazy

  $ hg debugchangelog --migrate doublewrite --unless lazy

  $ LOG=dag::protocol=debug hg log -Gr 'all()' -T '{desc} {remotenames}'
  DEBUG dag::protocol: resolve ids [4, 3, 1, 0] remotely
  o  B
  │
  │ o  C remote/master
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
  
Revlog -> Lazy:

  $ cd $TESTTMP/revlogrepo
  $ setconfig paths.default=test:server1

(Migration requires EdenAPI)

  $ hg debugchangelog --migrate lazy -v --config paths.default=$TESTTMP/a
  cannot migrate to lazy backend without edenapi

  $ hg debugchangelog --migrate lazy
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (lazytext):
    Local:
      Segments + IdMap: $TESTTMP/revlogrepo/.hg/store/segments/v1
      Zstore: $TESTTMP/revlogrepo/.hg/store/hgcommits/v1
      Revlog + Nodemap: (not used)
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Zstore (incomplete, draft)
      EdenAPI (remaining, public)
      Revlog (not used)
  Commit Hashes: lazy, using EdenAPI

--remove-backup removes backup files

  $ f .hg/store/00changelog.*
  .hg/store/00changelog.d
  .hg/store/00changelog.i
  .hg/store/00changelog.len
  $ ls .hg/store/segments
  v1
  v1.* (glob)

  $ hg debugchangelog --migrate lazy --remove-backup -v
  removed backup file 00changelog.d
  removed backup file 00changelog.i
  removed backup file 00changelog.len
  removed backup file segments/v1.* (glob) (?)

#if windows
  $ ls .hg/store/segments
  v1
  v1* (glob)
#else
  $ ls .hg/store/segments
  v1
#endif

Verify lazy changelog:

  $ hg verify
  commit graph passed quick local checks
  (pass --dag to perform slow checks with server)
  $ hg verify --dag
  commit graph passed quick local checks
  commit graph looks okay compared with the server

Revlog -> LazyText:

  $ cd $TESTTMP/revlogrepo2
  $ setconfig paths.default=test:server1

  $ hg debugchangelog --migrate lazytext
  $ hg debugchangelog --migrate lazytext
