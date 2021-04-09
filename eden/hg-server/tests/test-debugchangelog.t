#chg-compatible

  $ newrepo
  $ drawdag << 'EOS'
  > B C
  > |/|
  > A D
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
  │
  o  A
  

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
  o │  A
    │
    o  D
  

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
     Next Free Id: N4
     Segments: 1
      058c1e1fb10a+N0 : 417dca1c740d+N2 [] Root
   Level 0
    Group Master:
     Next Free Id: 0
     Segments: 0
    Group Non-Master:
     Next Free Id: N4
     Segments: 4
      112478962961+N3 : 112478962961+N3 [426bada5c675+N1]
      417dca1c740d+N2 : 417dca1c740d+N2 [058c1e1fb10a+N0, 426bada5c675+N1]
      426bada5c675+N1 : 426bada5c675+N1 [] Root
      058c1e1fb10a+N0 : 058c1e1fb10a+N0 [] Root

The segments backend does not need revlog data.

  $ rm -rf .hg/store/00changelog*
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  │ o  C
  ╭─┤
  o │  A
    │
    o  D
  

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
  o │  A
    │
    o  D
  
Cannot migrate hgsql repos

  $ echo hgsql >> .hg/requires

(filters out hgsql mysql import errors)
  $ hg debugchangelog --migrate revlog --config extensions.hgsql= --config hgsql.bypass=1 2>&1 | grep migrate
  abort: cannot migrate hgsql repo
