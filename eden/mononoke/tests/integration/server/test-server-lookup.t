# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

setup repo
  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B
  > # modify: A "a" "file_content"
  > # modify: B "b" "file_content"
  > # bookmark: B master_bookmark
  > # bookmark: A ffff775176ed42b1458a6281db4a0ccf4d9f287a
  > # author_date: A "1970-01-01T00:00:00+00:00"
  > # author_date: B "1970-01-01T00:00:00+00:00"
  > # author: A "test"
  > # author: B "test"
  > # message: A "a"
  > # message: B "a"
  > EOF
  A=63854830c9a9dc28e88ff155f2cb8bfebe6e8df0
  B=08c3b3bd5982552ae16fe66c208e885d1841fa0c

start mononoke
  $ start_and_wait_for_mononoke_server
  $ cd
  $ hg clone -q mono:repo client
  $ cd client
  $ hg up -q "min(all())"

Helper script to test the lookup function
  $ cat >> $TESTTMP/lookup.py <<EOF
  > from edenscm import registrar
  > from edenscm.node import bin
  > from edenscm import (bundle2, extensions)
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('lookup', [], ('key'))
  > def _lookup(ui, repo, key, **opts):
  >     with repo.connectionpool.get(ui.config("paths", "default")) as conn:
  >         remote = conn.peer
  >         csid = remote.lookup(key)
  >         if b'not found' in csid:
  >             print(csid)
  > EOF

Lookup non-existent hash
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup fffffffffffff6c66edf28380101a92122cbea50
  abort: fffffffffffff6c66edf28380101a92122cbea50 not found!
  [255]

Lookup existing hash
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup $A

Lookup non-existent bookmark
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup fake_bookmark
  abort: fake_bookmark not found!
  [255]

Lookup existing bookmark
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup master_bookmark

Lookup bookmark with hash name that exists as a hash (returns hash)
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup $B

Lookup bookmark with hash name that doesn't exist as a hash (returns bookmark -> hash)
  $ hg --config extensions.lookup=$TESTTMP/lookup.py lookup ffff775176ed42b1458a6281db4a0ccf4d9f287a
