#chg-compatible

Test that when lazy changelog is used, and the server strips some lazy portion
that is already present in the client-side, the client can still behave
gracefully.

  $ configure modern
  $ shorttraceback
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

  $ hg push test:e1 -r $E --to master --create -q

  $ hg clone -U test:e1 $TESTTMP/cloned1 -q

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

  $ hg push test:e2 -r $G --to master --create -q

Use the new server graph for lookup:

  $ cd $TESTTMP/cloned1
  $ setconfig paths.default=test:e2

Explicitly lookup the removed commit via edenapi:

  $ hg debugapi -e commithashtolocation -i "['$D']" -i "['$D']"
  error.HttpError: f585351a92f85104bff7c284233c338b10eb1df7 cannot be found
  [255]

Lookup commits that are removed:

  $ hg log -r $D -T '{desc}\n'
  error.HttpError: Server responded 404 Not Found for eager://$TESTTMP/e2/commit_revlog_data: f585351a92f85104bff7c284233c338b10eb1df7 cannot be found. Headers: {}
  [255]

Pull from the new server:

  $ hg pull
  pulling from test:e2
  failed to get fast pull data (9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found), using fallback path
  error.HttpError: 9bc730a19041f9ec7cb33c626e811aa233efb18c cannot be found
  [255]
