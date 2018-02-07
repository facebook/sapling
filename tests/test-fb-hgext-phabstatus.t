Setup

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../hgext/extlib/phabricator/arcconfig.py
  > phabstatus=
  > smartlog=
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

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}' > .arcconfig

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
  >   "result": [{"number": 1}]
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r . 2>&1 | grep KeyError
  KeyError: 'diff_status_name'

And finally, the success case

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": [{"number": 1, "diff_status_name": "Needs Review"}]
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{phabstatus}\n' -r .
  Needs Review

Make sure the code works without the smartlog extensions

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.querydiffhashes", {"revisionIDs": ["1"]}],
  >   "result": [{"number": 1, "diff_status_name": "Needs Review"}]
  > }]
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

  $ echo '{}' > .arcrc
  $ echo '{}' > .arcconfig
  $ hg log -T '{phabstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: arcrc is missing user credentials for host None.  use "arc install-certificate" to fix.
  Error

Make sure we get an error message if .arcrc is not proper JSON (for example
due to trailing commas). We do not use HG_ARC_CONDUIT_MOCK for this test,
in order for it to parse the badly formatted arc config file.

  $ echo '{,}' > ../.arcrc
  $ hg log -T '{phabstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: Configuration file $TESTTMP/.arcrc is not a proper JSON file.
  Error
