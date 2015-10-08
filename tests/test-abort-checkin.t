  $ cat > abortcommit.py <<EOF
  > from mercurial import error
  > def hook(**args):
  >     raise error.Abort("no commits allowed")
  > def reposetup(ui, repo):
  >     repo.ui.setconfig("hooks", "pretxncommit.nocommits", hook)
  > EOF
  $ abspath=`pwd`/abortcommit.py

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > abortcommit = $abspath
  > EOF

  $ hg init foo
  $ cd foo
  $ echo foo > foo
  $ hg add foo

mq may keep a reference to the repository so __del__ will not be
called and .hg/journal.dirstate will not be deleted:

  $ hg ci -m foo
  error: pretxncommit.nocommits hook failed: no commits allowed
  transaction abort!
  rollback completed
  abort: no commits allowed
  [255]
  $ hg ci -m foo
  error: pretxncommit.nocommits hook failed: no commits allowed
  transaction abort!
  rollback completed
  abort: no commits allowed
  [255]

  $ cd ..
