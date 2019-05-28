  $ setconfig extensions.treemanifest=!

Create a repository:

  $ hg config
  devel.all-warnings=true
  devel.default-date=0 0
  extensions.fsmonitor= (fsmonitor !)
  extensions.treemanifest=!
  fsmonitor.detectrace=1 (fsmonitor !)
  remotefilelog.reponame=reponame-default
  remotefilelog.cachepath=$TESTTMP/default-hgcache
  treemanifest.flatcompat=True
  ui.slash=True
  ui.interactive=False
  ui.mergemarkers=detailed
  ui.promptecho=True
  web.address=localhost
  web\.ipv6=(?:True|False) (re)
  $ hg init t
  $ cd t

Prepare a changeset:

  $ echo a > a
  $ hg add a

  $ hg status
  A a

Writes to stdio succeed and fail appropriately

#if devfull
  $ hg status 2>/dev/full
  A a

  $ hg status >/dev/full
  abort: No space left on device
  [255]
#endif

#if devfull no-chg
  $ hg status >/dev/full 2>&1
  [1]

  $ hg status ENOENT 2>/dev/full
  [1]
#endif

#if devfull chg
  $ hg status >/dev/full 2>&1
  [255]

  $ hg status ENOENT 2>/dev/full
  [255]
#endif

  $ hg commit -m test

This command is ancient:

  $ hg history
  changeset:   0:acb14030fe0a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Verify that updating to revision 0 via commands.update() works properly

  $ cat <<EOF > update_to_rev0.py
  > from edenscm.mercurial import ui, hg, commands
  > myui = ui.ui.load()
  > repo = hg.repository(myui, path='.')
  > commands.update(myui, repo, rev=0)
  > EOF
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ $PYTHON ./update_to_rev0.py
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg identify -n
  0


Poke around at hashes:

  $ hg manifest --debug
  b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3 644   a

  $ hg cat a
  a

Verify should succeed:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions

At the end...

  $ cd ..
