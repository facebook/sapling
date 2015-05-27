#require killdaemons

Test wire protocol unbundle with hashed heads (capability: unbundlehash)

  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > # This tests is intended for bundle1 only.
  > # bundle2 carries the head information inside the bundle itself and
  > # always uses 'force' as the heads value.
  > bundle2-exp = False
  > EOF

Create a remote repository.

  $ hg init remote
  $ hg serve -R remote --config web.push_ssl=False --config web.allow_push=* -p $HGPORT -d --pid-file=hg1.pid -E error.log -A access.log
  $ cat hg1.pid >> $DAEMON_PIDS

Clone the repository and push a change.

  $ hg clone http://localhost:$HGPORT/ local
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch local/README
  $ hg ci -R local -A -m hoge
  adding README
  $ hg push -R local
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

Ensure hashed heads format is used.
The hash here is always the same since the remote repository only has the null head.

  $ cat access.log | grep unbundle
  * - - [*] "POST /?cmd=unbundle HTTP/1.1" 200 - x-hgarg-1:heads=686173686564+6768033e216468247bd031a0a2d9876d79818f8f (glob)

Explicitly kill daemons to let the test exit on Windows

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

