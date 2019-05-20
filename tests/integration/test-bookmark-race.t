Test that bookmark updates during discovery don't cause problems for pulls
running concurrently. See the comment in mononoke/server/src/repo.rs:bundle2caps
for more.

  $ . $TESTDIR/library.sh

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

setup two repos: one will be used to pull into, and one will be used to
update master_bookmark concurrently.

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push
  $ cd repo-push
  $ hg up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ echo "b file content" > b
  $ hg add b
  $ hg ci -mb
  $ hgmn push ssh://user@dummy/repo -r .
  pushing to ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  updating bookmark master_bookmark
  $ echo "c file content" > c
  $ hg add c
  $ hg ci -mc

  $ cd $TESTTMP/repo-pull

configure an extension so that a push happens right after pulldiscovery

  $ cat > $TESTTMP/pulldiscovery_push.py << EOF
  > from edenscm.mercurial import (
  >     exchange,
  >     extensions,
  > )
  > def wrappulldiscovery(orig, pullop):
  >     print '*** starting discovery'
  >     orig(pullop)
  >     print '*** running push'
  >     pullop.repo.ui.system(
  >         "bash -c 'source $TESTDIR/library.sh; hgmn push -R $TESTTMP/repo-push ssh://user@dummy/repo'",
  >         onerr=lambda str: Exception(str),
  >     )
  >     print '*** push complete'
  > def extsetup(ui):
  >     extensions.wrapfunction(exchange, '_pulldiscovery', wrappulldiscovery)
  > EOF

  $ hgmn pull --config extensions.pulldiscovery_push=$TESTTMP/pulldiscovery_push.py
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  *** starting discovery
  searching for changes
  *** running push
  pushing to ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  updating bookmark master_bookmark
  *** push complete
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets e2750f699c89

  $ hg bookmarks
     master_bookmark           1:e2750f699c89

pull again to ensure the new version makes it into repo-pull

  $ hgmn pull
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  new changesets e5c8b04bf9a0
  $ hg bookmarks
     master_bookmark           2:e5c8b04bf9a0
