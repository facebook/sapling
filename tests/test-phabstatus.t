Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../phabricator/arcconfig.py
  > phabstatus=$TESTDIR/../phabstatus.py
  > smartlog=$TESTDIR/../smartlog.py
  > EOF
  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg ci -qAm 'Differential Revision: https://phabricator.fb.com/D1'

With an invalid arc configuration

  $ hg log -T '{phabstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: no .arcconfig foundError

Configure arc...

  $ echo '{}' > .arcconfig
  $ echo '{}' > .arcrc

And now with bad responses:

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": {}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "error_info": "failed, yo"}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yoError

Missing id field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}],
  >   "result": [{"statusName": "Needs Review"}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error

And finally, the success case

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}],
  >   "result": [{"id": 1, "statusName": "Needs Review"}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Needs Review

