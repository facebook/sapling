bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

This test only works on hg 1.7 and later
  $ python -c 'from mercurial import util ; assert \
  >  util.version() != "unknown" and util.version() > "1.7"' || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error"
  >     count=`expr $count + 1`
  > }
  $ hgcommit()
  > {
  >     HGDATE="2007-01-01 00:00:$count +0000"
  >     hg commit -d "$HGDATE" "$@" >/dev/null 2>/dev/null || echo "hg commit error"
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/

  $ echo alpha > alpha
  $ git add alpha
  $ commit -m "add alpha"

  $ cd ..
  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd hgrepo
  $ hg incoming | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo

  $ cd ../gitrepo
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'

  $ cd ../hgrepo
  $ hg incoming | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo
  changeset:   1:9497a4ee62e1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  

  $ cd ../gitrepo
  $ git checkout -b b1 HEAD^
  Switched to a new branch 'b1'
  $ mkdir d
  $ echo gamma > d/gamma
  $ git add d/gamma
  $ commit -m'add d/gamma'
  $ git tag t1

  $ echo gamma 2 >> d/gamma
  $ git add d/gamma
  $ commit -m'add d/gamma line 2'

  $ cd ../hgrepo
  $ hg incoming -p | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo
  changeset:   1:9497a4ee62e1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  diff -r 3442585be8a6 -r 9497a4ee62e1 beta
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/beta	Mon Jan 01 00:00:11 2007 +0000
  @@ -0,0 +1,1 @@
  +beta
  
  changeset:   2:9865e289be73
  tag:         t1
  parent:      0:3442585be8a6
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add d/gamma
  
  diff -r 3442585be8a6 -r 9865e289be73 d/gamma
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d/gamma	Mon Jan 01 00:00:12 2007 +0000
  @@ -0,0 +1,1 @@
  +gamma
  
  changeset:   3:5202f48c20c9
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     add d/gamma line 2
  
  diff -r 9865e289be73 -r 5202f48c20c9 d/gamma
  --- a/d/gamma	Mon Jan 01 00:00:12 2007 +0000
  +++ b/d/gamma	Mon Jan 01 00:00:13 2007 +0000
  @@ -1,1 +1,2 @@
   gamma
  +gamma 2
  

  $ echo % incoming -r
  % incoming -r
  $ hg incoming -r master | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo
  changeset:   1:9497a4ee62e1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:11 2007 +0000
  summary:     add beta
  
  $ hg incoming -r b1 | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo
  changeset:   1:9865e289be73
  tag:         t1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add d/gamma
  
  changeset:   2:5202f48c20c9
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:13 2007 +0000
  summary:     add d/gamma line 2
  
  $ hg incoming -r t1 | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo
  changeset:   1:9865e289be73
  tag:         t1
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:12 2007 +0000
  summary:     add d/gamma
  

  $ echo % nothing incoming after pull
  % nothing incoming after pull
"adding remote bookmark" message was added in Mercurial 2.3
  $ hg pull | grep -v "adding remote bookmark"
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg incoming | grep -v 'no changes found' | grep -v 'bookmark:'
  comparing with $TESTTMP/gitrepo

  $ echo 'done'
  done
