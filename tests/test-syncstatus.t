Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../phabricator/arcconfig.py
  > phabstatus=$TESTDIR/../hgext3rd/phabstatus.py
  > smartlog=$TESTDIR/../hgext3rd/smartlog.py
  > EOF
  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg ci -qAm 'Differential Revision: https://phabricator.fb.com/D1'

With an invalid arc configuration

  $ hg log -T '{syncstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: no .arcconfig foundError

Configure arc...

  $ echo '{}' > .arcconfig
  $ echo '{}' > .arcrc

And now with bad responses:

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}], "result": {}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}], "error_info": "failed, yo"}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yoError

Missing status field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"hash": "this is the best hash ewa", "count" : "3"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error

Missing count field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"hash": "this is the best hash ewa", "status" : "Committed"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error

Missing hash field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"status": "Needs Review", "count" : "3"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error

And finally, the success case

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"count": 3, "status": "Committed", "hash": "lolwut"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  committed

