
  $ "$TESTDIR/hghave" git || exit 80
  $ echo "[core]" >> $HOME/.gitconfig
  $ echo "autocrlf = false" >> $HOME/.gitconfig
  $ echo "[core]" >> $HOME/.gitconfig
  $ echo "autocrlf = false" >> $HOME/.gitconfig
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ echo '[convert]' >> $HGRCPATH
  $ echo 'hg.usebranchnames = True' >> $HGRCPATH
  $ echo 'hg.tagsbranch = tags-update' >> $HGRCPATH
  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE
  $ count=10
  $ action()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git "$@" >/dev/null 2>/dev/null || echo "git command error"
  >     count=`expr $count + 1`
  > }
  $ glog()
  > {
  >     hg glog --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
  > }
  $ convertrepo()
  > {
  >     hg convert --datesort git-repo hg-repo
  > }

Build a GIT repo with at least 1 tag

  $ mkdir git-repo
  $ cd git-repo
  $ git init >/dev/null 2>&1
  $ echo a > a
  $ git add a
  $ action commit -m "rev1"
  $ action tag -m "tag1" tag1
  $ cd ..

Do a first conversion

  $ convertrepo
  initializing destination hg-repo repository
  scanning source...
  sorting...
  converting...
  0 rev1
  updating tags
  updating bookmarks

Simulate upstream  updates after first conversion

  $ cd git-repo
  $ echo b > a
  $ git add a
  $ action commit -m "rev2"
  $ action tag -m "tag2" tag2
  $ cd ..

Perform an incremental conversion

  $ convertrepo
  scanning source...
  sorting...
  converting...
  0 rev2
  updating tags
  updating bookmarks

Print the log

  $ cd hg-repo
  $ glog
  o  3 "update tags" files: .hgtags
  |
  | o  2 "rev2" files: a
  | |
  o |  1 "update tags" files: .hgtags
   /
  o  0 "rev1" files: a
  

  $ cd ..
