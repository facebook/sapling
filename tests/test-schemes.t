
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > schemes=
  > 
  > [schemes]
  > l = http://localhost:$HGPORT/
  > parts = http://{1}:$HGPORT/
  > z = file:\$PWD/
  > EOF
  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Am initial
  adding a
  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg incoming l://
  comparing with l://
  searching for changes
  no changes found
  [1]

check that {1} syntax works

  $ hg incoming --debug parts://localhost
  using http://localhost:$HGPORT/
  sending between command
  comparing with parts://localhost
  sending heads command
  searching for changes
  no changes found
  [1]

check that paths are expanded

  $ PWD=`pwd` hg incoming z://
  comparing with z://
  searching for changes
  no changes found
  [1]

errors

  $ cat errors.log
