#require serve

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

invalid scheme

  $ hg log -R z:z
  abort: no '://' in scheme url 'z:z'
  [255]

http scheme

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
  sending capabilities command
  comparing with parts://localhost/
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  no changes found
  [1]

check that paths are expanded

  $ PWD=`pwd` hg incoming z://
  comparing with z://
  searching for changes
  no changes found
  [1]

check that debugexpandscheme outputs the canonical form

  $ hg debugexpandscheme bb://user/repo
  https://bitbucket.org/user/repo

expanding an unknown scheme emits the input

  $ hg debugexpandscheme foobar://this/that
  foobar://this/that

expanding a canonical URL emits the input

  $ hg debugexpandscheme https://bitbucket.org/user/repo
  https://bitbucket.org/user/repo

errors

  $ cat errors.log

  $ cd ..
