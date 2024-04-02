#require git no-windows no-eden
#debugruntest-compatible

  $ . $TESTDIR/git.sh

Initialize the server repos.

  $ git init -q --bare -b main repo-1.git
  $ git init -q --bare -b main repo-2.git

Initialize the Sapling repo.

  $ hg clone -q --git "$TESTTMP/repo-1.git" client-repo
  $ cd client-repo
  $ hg paths --add default-push "$TESTTMP/repo-2.git"
  $ touch testfile
  $ hg add testfile
  $ hg commit testfile -m testcommit

Pushing without specifying a path pushes to the 'default-push' path.

  $ hg push -q -r . --to main --create

  $ GIT_DIR="$TESTTMP/repo-1.git" git log --pretty=format:%s%n
  fatal: your current branch 'main' does not have any commits yet
  [128]

  $ GIT_DIR="$TESTTMP/repo-2.git" git log --pretty=format:%s%n
  testcommit

After deleting the 'default-push' path,
pushing without specifying a path pushes to the 'default' path

  $ hg paths --delete default-push
  $ hg push -q -r . --to main --create

  $ GIT_DIR="$TESTTMP/repo-1.git" git log --pretty=format:%s%n
  testcommit

  $ GIT_DIR="$TESTTMP/repo-2.git" git log --pretty=format:%s%n
  testcommit
