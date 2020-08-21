
  $ export GIT_AUTHOR_NAME='test'
  $ export GIT_AUTHOR_EMAIL='test@example.org'
  $ export GIT_AUTHOR_DATE="2007-01-01 00:00:10 +0000"
  $ export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
  $ export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
  $ export GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ echo 2 > beta
  $ git add beta
  $ git commit -q -mbeta

Init an hg repo using the git changelog backend:

  $ cd $TESTTMP
  $ hg debuginitgit --git-dir gitrepo/.git repo1
  $ cd repo1

  $ hg log -Gr 'all()' -T '{node} {desc}'
  o  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  |
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (segmented git):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Git: $TESTTMP/gitrepo/.git
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Git

Migrate to revlog changelog format:

  $ hg debugchangelog --migrate rustrevlog
  $ hg log -Gr 'all()' -T '{node} {desc}'
  o  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  |
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
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
