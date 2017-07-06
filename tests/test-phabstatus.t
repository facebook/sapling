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

  $ hg log -T '{phabstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: no .arcconfig found
  Error

Configure arc...

  $ echo '{}' > .arcconfig
  $ echo '{}' > .arcrc

And now with bad responses:

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}], "result": {}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}], "error_info": "failed, yo"}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yo
  Error

Missing status field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"hash": "this is the best hash ewa"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Error

And finally, the success case

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"count": 1, "status": "Needs Review", "hash": "lolwut"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Needs Review

Make sure the code works without the smartlog extensions

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": {"1" : {"count": 1, "status": "Needs Review", "hash": "lolwut"}}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg --config 'extensions.smartlog=!' log -T '{phabstatus}\n' -r .
  Needs Review

Make sure the template keywords are documented correctly

  $ hg help templates | egrep 'phabstatus|syncstatus'
      phabstatus    String. Return the diff approval status for a given hg rev
      syncstatus    String. Return whether the local revision is in sync with

Make sure we get decent error messages when .arcrc is missing credential
information.  We intentionally do not use HG_ARC_CONDUIT_MOCK for this test,
so it tries to parse the (empty) arc config files.

  $ hg log -T '{phabstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: arcrc is missing user credentials for host https://phabricator.intern.facebook.com/api/.  use "arc install-certificate" to fix.
  Error
