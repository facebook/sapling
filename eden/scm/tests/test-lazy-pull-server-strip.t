#chg-compatible
#require no-eden

Test that when lazy changelog is used, and the server strips some lazy portion
that is already present in the client-side, the client can still behave
gracefully.

  $ configure modern
  $ setconfig ui.ssh=false

Prepare repo:

  $ newremoterepo before-truncate
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ sl push test:e1 -r $E --to master --create -q

  $ sl clone -U test:e1 $TESTTMP/cloned1 -q

Truncate history server side by rebuilding the segmented changelog graph without D, E:

  $ newremoterepo after-truncate
  $ drawdag << 'EOS'
  > G
  > |
  > F
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ sl push test:e2 -r $G --to master --create -q

Use the new server graph for lookup:

  $ cd $TESTTMP/cloned1
  $ setconfig paths.default=test:e2

Explicitly lookup the removed commit via edenapi:

  $ sl debugapi -e commithashtolocation -i "['$D']" -i "['$D']"
  abort: f585351a92f85104bff7c284233c338b10eb1df7 cannot be found
  [255]

Lookup commits that are removed:

  $ echo $D
  f585351a92f85104bff7c284233c338b10eb1df7
  $ echo $E
  9bc730a19041f9ec7cb33c626e811aa233efb18c

  $ sl log -r $D -T '{desc}\n' --config devel.collapse-traceback=0 > log.out 2>&1; [ $? -ne 0 ] && echo failed
  failed
  $ grep 'error.HttpError: 9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found' log.out
  error.HttpError: 9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found

Pull from the new server:

  $ sl pull > pull.out 2>&1; [ $? -ne 0 ] && echo failed
  failed
  $ grep 'pulling from test:e2' pull.out
  pulling from test:e2
  $ grep 'failed to get fast pull data (9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found), using fallback path' pull.out
  failed to get fast pull data (9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found), using fallback path
  $ grep 'error.HttpError: 9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found' pull.out
  error.HttpError: 9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found
