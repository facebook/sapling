# Fails for some reason, need to investigate
#   $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

bail early if the user is already running git-daemon
  $ ! (echo hi | nc localhost 9418 2>/dev/null) || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ echo 'hgext.bookmarks =' >> $HGRCPATH

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
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error" | sed 's/, 0 deletions(-)//'
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo1
  $ cd gitrepo1
  $ git init | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitrepo1/.git/
  
  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add alpha'
  $ cd ..

  $ mkdir gitsubrepo
  $ cd gitsubrepo
  $ git init | python -c "import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
  Initialized empty Git repository in gitsubrepo/.git/
  
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'
  $ cd ..

  $ mkdir gitrepo2
  $ cd gitrepo2

  $ rmpwd="import sys; print sys.stdin.read().replace('$(dirname $(pwd))/', '')"
different versions of git spell the dir differently. Older versions
use the full path to the directory all the time, whereas newer
version spell it sanely as it was given (eg . in a newer version,
while older git will use the full normalized path for .)
  $ clonefilt='s/Cloning into/Initialized empty Git repository in/;s/in .*/in .../'

  $ git clone ../gitrepo1 . | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git submodule add ../gitsubrepo subrepo | python -c "$rmpwd" | sed "$clonefilt" | egrep -v '^done\.$'
  Initialized empty Git repository in ...
  
  $ git commit -m 'add subrepo' | sed 's/, 0 deletions(-)//'
  [master e42b08b] add subrepo
   2 files changed, 4 insertions(+)
   create mode 100644 .gitmodules
   create mode 160000 subrepo
  $ git rm --cached subrepo
  rm 'subrepo'
  $ git rm .gitmodules
  rm '.gitmodules'
  $ git commit -m 'rm subrepo' | sed 's/, 0 deletions(-)//' | sed 's/, 0 insertions(+)//'
  [master 7e4c934] rm subrepo
   2 files changed, 4 deletions(-)
   delete mode 100644 .gitmodules
   delete mode 160000 subrepo
  $ cd ..

  $ hg clone gitrepo2 hgrepo | grep -v '^updating'
  importing git objects into hg
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ hg log --graph  | grep -v ': *master'
  @  changeset:   2:954cdf1c8c82
  |  tag:         default/master
  |  tag:         tip
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     rm subrepo
  |
  o  changeset:   1:145121c71064
  |  user:        test <test@example.org>
  |  date:        Mon Jan 01 00:00:11 2007 +0000
  |  summary:     add subrepo
  |
  o  changeset:   0:3442585be8a6
     user:        test <test@example.org>
     date:        Mon Jan 01 00:00:10 2007 +0000
     summary:     add alpha
  

  $ echo % we should have some bookmarks
  % we should have some bookmarks
  $ hg book
   * master                    2:954cdf1c8c82
