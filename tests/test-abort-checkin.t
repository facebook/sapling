  $ cat > abortcommit.py <<EOF
  > from mercurial import util
  > def hook(**args):
  >     raise util.Abort("no commits allowed")
  > def reposetup(ui, repo):
  >     repo.ui.setconfig("hooks", "pretxncommit.nocommits", hook)
  > EOF
  $ abspath=`pwd`/abortcommit.py

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "abortcommit = $abspath" >> $HGRCPATH

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
