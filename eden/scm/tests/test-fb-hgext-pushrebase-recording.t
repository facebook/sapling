#chg-compatible

TODO: Make this test compatibile with obsstore enabled.
  $ disable treemanifest
  $ setconfig experimental.evolution=
  $ . helpers-usechg.sh

  $ . "$TESTDIR/library.sh"
  $ getmysqldb
  $ createpushrebaserecordingdb

Setup

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF

  $ commit() {
  >   hg commit -d "0 0" -A -m "$@"
  > }

  $ log() {
  >   hg log -G -T "{desc} [{phase}:{node|short}] {bookmarks}" "$@"
  > }

Set up server repository

  $ hg init server
  $ cd server
  $ echo foo > a
  $ echo foo > b
  $ commit 'initial'
  adding a
  adding b

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ hg clone ssh://user@dummy/server server2 -q
  $ hg clone ssh://user@dummy/server2 client2 -q
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc

  $ cd ../server
  $ echo 'bar' > a
  $ commit 'a => bar'

  $ cd ../client
  $ hg rm b
  $ commit 'b => xxx'

Non-conflicting commit should be accepted

  $ cd ../server2
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc

  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase =" >> .hg/hgrc
  $ cat >> $TESTTMP/uploader.sh <<EOF
  > #! /bin/bash
  > cp \$1 $TESTTMP/bundle
  > printf handle
  > EOF
  $ chmod +x $TESTTMP/uploader.sh

  $ cat >> .hg/hgrc <<EOF
  > [pushrebase]
  > bundlepartuploadbinary=$TESTTMP/uploader.sh {filename}
  > enablerecording=True
  > recordingsqlargs=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS
  > recordingrepoid=42
  > EOF

  $ log
  @  a => bar [draft:add0c792bfce]
  |
  o  initial [draft:2bb9d20e471c]
   (re)
  $ cd ../client
  $ log
  @  b => xxx [draft:46a2df24e272]
  |
  o  initial [public:2bb9d20e471c]
   (re)
  $ hg log -r . -T '{node}\n'
  46a2df24e27273bb06dbf28b085fcc2e911bf986
  $ hg push -r . --to default -q

Check that new entry was added to the db
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL

Check that bundle was created, then try to send it from client2 to server2 using
unbundle method to make sure that bundle is valid
  $ ls $TESTTMP/bundle
  $TESTTMP/bundle

  $ cd ../client2

  $ cat >> $TESTTMP/unbundle.py <<EOF
  > from edenscm.mercurial import registrar
  > from edenscm.mercurial import (bundle2, extensions)
  > from edenscm.mercurial.node import bin, hex
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('sendunbundle', [
  >     ('f', 'file', '', 'specify the file', 'FILE'),
  > ], '')
  > def _unbundle(ui, repo, **opts):
  >     f = opts.get('file')
  >     f = open(f)
  >     with repo.connectionpool.get("ssh://user@dummy/server2") as conn:
  >         remote = conn.peer
  >         bundle = remote.unbundle(f, ["force"], "url")
  >         for part in bundle.iterparts():
  >           print(len(list(part.read())))
  > EOF

  $ cd ../server2
  $ log
  @  initial [public:2bb9d20e471c]
   (re)
  $ cd ../client2
  $ hg sendunbundle --config extensions.unbundle=$TESTTMP/unbundle.py --file $TESTTMP/bundle
  remote: pushing 1 changeset:
  remote:     46a2df24e272  b => xxx
  $ cd ../server2
  $ log
  o  b => xxx [public:46a2df24e272]
  |
  @  initial [public:2bb9d20e471c]
   (re)

Push conflicting changes
  $ cd ../client
  $ hg up -q 1 && echo 'baz' > a && hg commit -Am 'conflict commit'
  $ hg push -r . --to default
  pushing to ssh://user@dummy/server
  searching for changes
  remote: conflicting changes in:
      a
      b
  remote: (pull and rebase your changes locally, then try again)
  abort: push failed on remote
  [255]
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	a\nb	NULL
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select timestamps from pushrebaserecording'
  timestamps
  {"46a2df24e27273bb06dbf28b085fcc2e911bf986": [0.0, 0]}
   (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select recorded_manifest_hashes from pushrebaserecording'
  recorded_manifest_hashes
  {"46a2df24e27273bb06dbf28b085fcc2e911bf986": "adfcaf0f4d07a35613ae31df578e38305893406d"}
   (re)

Create two commits and push them
  $ cd ../client
  $ hg up -q 0
  $ echo stack1 > stack1 && hg add stack1 && hg ci -m stack1
  $ echo stack2 > stack2 && hg add stack2 && hg ci -m stack2
  $ log -r '.^::.'
  @  stack2 [draft:359a44f39821]
  |
  o  stack1 [draft:36638495eb5c]
  |
  ~
  $ hg push -r . --to default -q
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select timestamps from pushrebaserecording' | grep 359a44f39821 | wc -l
  \s*1 (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select timestamps from pushrebaserecording' | grep 36638495eb5c | wc -l
  \s*1 (re)

Make sure that we don't record anything on non-pushrebase push
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  3
  $ hg up -q 0
  $ echo stack1 > stack1 && hg add stack1 && hg ci -m stack1
  $ hg push --force
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 4 changesets with 1 changes to 3 files
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  3

Disable trystackpush
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  3
  $ cd ../server
  $ hg book master -r 359a44f39
  $ setconfig pushrebase.trystackpush=False
  $ cd -
  $TESTTMP/client
  $ hg up -q 359a44f39 && echo '' > newfile && hg commit -Am 'no trystackpush commit'
  adding newfile
  $ hg push -r . --to master -q

Check that new entry was added to the db
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	a\nb	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	038f4259f9592754a8b7089b921e2df9d50bbf95	default	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	NULL


Enable pretxnchangegroup hooks and make sure we record failed pushes in that case
  $ cd ../server
  $ setconfig hooks.pretxnchangegroup=false

  $ cd -
  $TESTTMP/client
  $ hg up -q tip
  $ echo '' > failedhook && hg commit -Am 'hook will fail'
  adding failedhook
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  4
  $ hg push -r . --to master
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 changeset:
  remote:     e42da70f7a80  hook will fail
  remote: transaction abort!
  remote: rollback completed
  remote: pretxnchangegroup hook exited with status 1
  abort: push failed on remote
  [255]
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	a\nb	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	038f4259f9592754a8b7089b921e2df9d50bbf95	default	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	pretxnchangegroup hook exited with status 1

Enable prepushrebase hooks and make sure we record failed pushes in that case
  $ cd ../server
  $ setconfig hooks.prepushrebase=false

  $ cd -
  $TESTTMP/client
  $ hg up -q tip
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  5
  $ hg push -r . --to master
  pushing to ssh://user@dummy/server
  searching for changes
  remote: prepushrebase hook exited with status 1
  abort: push failed on remote
  [255]
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	a\nb	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	038f4259f9592754a8b7089b921e2df9d50bbf95	default	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	pretxnchangegroup hook exited with status 1
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	prepushrebase hook exited with status 1

Run python hook that records hook failure reason
  $ cd ../server
  $ cat >> $TESTTMP/hook.py <<EOF
  > from edenscm.mercurial import error
  > def fail(ui, repo, *args, **kwargs):
  >   e = error.HookAbort("failure")
  >   e.reason = "reason for failure"
  >   raise e
  > EOF
  $ setconfig hooks.prepushrebase=python:$TESTTMP/hook.py:fail
  $ cd -
  $TESTTMP/client
  $ hg up -q tip
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select count(*) from pushrebaserecording'
  count(*)
  6
  $ hg push -r . --to master
  pushing to ssh://user@dummy/server
  searching for changes
  remote: error: prepushrebase hook failed: failure
  remote: failure
  abort: push failed on remote
  [255]
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'select repo_id, ontorev, onto_rebased_rev, onto, bundlehandle, conflicts, pushrebase_errmsg from pushrebaserecording'
  repo_id	ontorev	onto_rebased_rev	onto	bundlehandle	conflicts	pushrebase_errmsg
  42	add0c792bfce89610d277fd5b1e32f5287994d1d	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	NULL	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	6a6d9484552c82e5f21b4ed4fce375930812f88c	default	handle	a\nb	NULL
  42	6a6d9484552c82e5f21b4ed4fce375930812f88c	038f4259f9592754a8b7089b921e2df9d50bbf95	default	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	NULL
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	pretxnchangegroup hook exited with status 1
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	prepushrebase hook exited with status 1
  42	359a44f39821c5c43f4506f79511e91f42d8b7af	359a44f39821c5c43f4506f79511e91f42d8b7af	master	handle	NULL	failure reason: reason for failure
