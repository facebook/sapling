#require chg

  $ cp $HGRCPATH $HGRCPATH.orig

init repo

  $ chg init foo
  $ cd foo

ill-formed config

  $ chg status
  $ echo '=brokenconfig' >> $HGRCPATH
  $ chg status
  hg: parse error at * (glob)
  [255]

  $ cp $HGRCPATH.orig $HGRCPATH

long socket path

  $ sockpath=$TESTTMP/this/path/should/be/longer/than/one-hundred-and-seven/characters/where/107/is/the/typical/size/limit/of/unix-domain-socket
  $ mkdir -p $sockpath
  $ bakchgsockname=$CHGSOCKNAME
  $ CHGSOCKNAME=$sockpath/server
  $ export CHGSOCKNAME
  $ chg root
  $TESTTMP/foo
  $ rm -rf $sockpath
  $ CHGSOCKNAME=$bakchgsockname
  $ export CHGSOCKNAME

  $ cd ..

editor
------

  $ cat >> pushbuffer.py <<EOF
  > def reposetup(ui, repo):
  >     repo.ui.pushbuffer(subproc=True)
  > EOF

  $ chg init editor
  $ cd editor

by default, system() should be redirected to the client:

  $ touch foo
  $ CHGDEBUG= HGEDITOR=cat chg ci -Am channeled --edit 2>&1 \
  > | egrep "HG:|run 'cat"
  chg: debug: * run 'cat "*"' at '$TESTTMP/editor' (glob)
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added foo

but no redirection should be made if output is captured:

  $ touch bar
  $ CHGDEBUG= HGEDITOR=cat chg ci -Am bufferred --edit \
  > --config extensions.pushbuffer="$TESTTMP/pushbuffer.py" 2>&1 \
  > | egrep "HG:|run 'cat"
  [1]

check that commit commands succeeded:

  $ hg log -T '{rev}:{desc}\n'
  1:bufferred
  0:channeled

  $ cd ..

pager
-----

  $ cat >> fakepager.py <<EOF
  > import sys
  > for line in sys.stdin:
  >     sys.stdout.write('paged! %r\n' % line)
  > EOF

enable pager extension globally, but spawns the master server with no tty:

  $ chg init pager
  $ cd pager
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pager =
  > [pager]
  > pager = $PYTHON $TESTTMP/fakepager.py
  > EOF
  $ chg version > /dev/null
  $ touch foo
  $ chg ci -qAm foo

pager should be enabled if the attached client has a tty:

  $ chg log -l1 -q --config ui.formatted=True
  paged! '0:1f7b0de80e11\n'
  $ chg log -l1 -q --config ui.formatted=False
  0:1f7b0de80e11

chg waits for pager if runcommand raises

  $ cat > $TESTTMP/crash.py <<EOF
  > from mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'crash')
  > def pagercrash(ui, repo, *pats, **opts):
  >     ui.write('going to crash\n')
  >     raise Exception('.')
  > EOF

  $ cat > $TESTTMP/fakepager.py <<EOF
  > from __future__ import absolute_import
  > import sys
  > import time
  > for line in iter(sys.stdin.readline, ''):
  >     if 'crash' in line: # only interested in lines containing 'crash'
  >         # if chg exits when pager is sleeping (incorrectly), the output
  >         # will be captured by the next test case
  >         time.sleep(1)
  >         sys.stdout.write('crash-pager: %s' % line)
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > crash = $TESTTMP/crash.py
  > EOF

  $ chg crash --pager=on --config ui.formatted=True 2>/dev/null
  crash-pager: going to crash
  [255]

  $ cd ..

server lifecycle
----------------

chg server should be restarted on code change, and old server will shut down
automatically. In this test, we use the following time parameters:

 - "sleep 1" to make mtime different
 - "sleep 2" to notice mtime change (polling interval is 1 sec)

set up repository with an extension:

  $ chg init extreload
  $ cd extreload
  $ touch dummyext.py
  $ cat <<EOF >> .hg/hgrc
  > [extensions]
  > dummyext = dummyext.py
  > EOF

isolate socket directory for stable result:

  $ OLDCHGSOCKNAME=$CHGSOCKNAME
  $ mkdir chgsock
  $ CHGSOCKNAME=`pwd`/chgsock/server

warm up server:

  $ CHGDEBUG= chg log 2>&1 | egrep 'instruction|start'
  chg: debug: * start cmdserver at $TESTTMP/extreload/chgsock/server.* (glob)

new server should be started if extension modified:

  $ sleep 1
  $ touch dummyext.py
  $ CHGDEBUG= chg log 2>&1 | egrep 'instruction|start'
  chg: debug: * instruction: unlink $TESTTMP/extreload/chgsock/server-* (glob)
  chg: debug: * instruction: reconnect (glob)
  chg: debug: * start cmdserver at $TESTTMP/extreload/chgsock/server.* (glob)

old server will shut down, while new server should still be reachable:

  $ sleep 2
  $ CHGDEBUG= chg log 2>&1 | (egrep 'instruction|start' || true)

socket file should never be unlinked by old server:
(simulates unowned socket by updating mtime, which makes sure server exits
at polling cycle)

  $ ls chgsock/server-*
  chgsock/server-* (glob)
  $ touch chgsock/server-*
  $ sleep 2
  $ ls chgsock/server-*
  chgsock/server-* (glob)

since no server is reachable from socket file, new server should be started:
(this test makes sure that old server shut down automatically)

  $ CHGDEBUG= chg log 2>&1 | egrep 'instruction|start'
  chg: debug: * start cmdserver at $TESTTMP/extreload/chgsock/server.* (glob)

shut down servers and restore environment:

  $ rm -R chgsock
  $ CHGSOCKNAME=$OLDCHGSOCKNAME
  $ cd ..
