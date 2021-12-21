#chg-compatible

  $ export GIT_AUTHOR_NAME='test'
  $ export GIT_AUTHOR_EMAIL='test@example.org'
  $ export GIT_AUTHOR_DATE="2007-01-01 00:00:10 +0000"
  $ export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
  $ export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
  $ export GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
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
  │
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

Test checkout:

  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo *
  alpha beta
  $ cat beta
  2

Test diff:

  $ hg log -r tip -p --config diff.git=1
  commit:      3f5848713286
  bookmark:    master
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     beta
  
  diff --git a/beta b/beta
  new file mode 100644
  --- /dev/null
  +++ b/beta
  @@ -0,0 +1,1 @@
  +2
  
Test status:

  $ hg status
  $ echo 3 > alpha
  $ hg status
  M alpha
