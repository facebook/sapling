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
  |\
  +---o  B
  | |
  | o  D
  |
  o  A
  
