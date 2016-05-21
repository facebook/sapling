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

alias having an environment variable and set to use pager

  $ rm $HGRCPATH
  $ cat >> $HGRCPATH <<'EOF'
  > [ui]
  > formatted = yes
  > [extensions]
  > pager =
  > [pager]
  > pager = sed -e 's/^/P/'
  > attend = printa
  > [alias]
  > printa = log -T "$A\n" -r 0
  > EOF

  $ A=1 chg printa
  P1
  $ A=2 chg printa
  P2

  $ cp $HGRCPATH.orig $HGRCPATH
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
  chg: debug: start cmdserver at $TESTTMP/extreload/chgsock/server

new server should be started if extension modified:

  $ sleep 1
  $ touch dummyext.py
  $ CHGDEBUG= chg log 2>&1 | egrep 'instruction|start'
  chg: debug: instruction: unlink $TESTTMP/extreload/chgsock/server-* (glob)
  chg: debug: instruction: reconnect
  chg: debug: start cmdserver at $TESTTMP/extreload/chgsock/server

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
  chg: debug: start cmdserver at $TESTTMP/extreload/chgsock/server

shut down servers and restore environment:

  $ rm -R chgsock
  $ CHGSOCKNAME=$OLDCHGSOCKNAME
  $ cd ..
