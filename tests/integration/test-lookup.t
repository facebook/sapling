  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_mononoke_config

setup repo
  $ cd $TESTTMP
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ touch b
  $ hg add b
  $ hg ci -ma
  $ hg log
  changeset:   1:f9ae6ef0865e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

setup master bookmark
  $ hg bookmark master_bookmark -r 3903775176ed
  $ hg bookmark ffff775176ed42b1458a6281db4a0ccf4d9f287a
  $ hg bookmark 3e19bf519e9af6c66edf28380101a92122cbea50

blobimport
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd repo-hg
  $ hg up -q 0

Helper script to test the lookup function
  $ cat >> $TESTTMP/lookup.py <<EOF
  > from edenscm.mercurial import registrar
  > from edenscm.mercurial.node import bin
  > from edenscm.mercurial import (bundle2, extensions)
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('lookup', [], ('key'))
  > def _lookup(ui, repo, key, **opts):
  >     treemanifestext = extensions.find('treemanifest')
  >     fallbackpath = treemanifestext.getfallbackpath(repo)
  >     with repo.connectionpool.get(fallbackpath) as conn:
  >         remote = conn.peer
  >         csid = remote.lookup(key)
  >         if b'not found' in csid:
  >             print(csid)
  > EOF

Lookup non-existent hash
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup fffffffffffff6c66edf28380101a92122cbea50
  abort: fffffffffffff6c66edf28380101a92122cbea50 not found!
  [255]

Lookup existing hash
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup f9ae6ef0865e00431f2af076be6b680f75dd2777

Lookup non-existent bookmark
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup fake_bookmark
  abort: fake_bookmark not found!
  [255]

Lookup existing bookmark
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup master_bookmark

Lookup bookmark with hash name that exists as a hash (returns hash)
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup 3903775176ed42b1458a6281db4a0ccf4d9f287a

Lookup bookmark with hash name that doesn't exist as a hash (returns bookmark -> hash)
  $ hgmn --config extensions.lookup=$TESTTMP/lookup.py lookup ffff775176ed42b1458a6281db4a0ccf4d9f287a
