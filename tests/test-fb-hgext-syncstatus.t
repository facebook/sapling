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

  $ hg log -T '{syncstatus}\n' -r .
  arcconfig configuration problem. No diff information can be provided.
  Error info: no .arcconfig found
  Error

Configure arc...

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}' > .arcconfig

And now with bad responses:

  $ cat > $TESTTMP/mockduit << EOF
  > [{}]
  > EOF
  $ OVERRIDE_GRAPHQL_URI=https://a.com HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error

  $ cat > $TESTTMP/mockduit << EOF
  > [{"error_info": "failed, yo"}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  Error talking to phabricator. No diff information can be provided.
  Error info: failed, yo
  Error

Missing status field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [[{
  >   "number": 1,
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3}
  > }]]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r . 2>&1 | grep Error
  KeyError: 'diff_status_name'

Missing count field is treated as an error

  $ cat > $TESTTMP/mockduit << EOF
  > [[{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   }
  > }]]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r . 2>&1 | grep Error
  KeyError: 'differential_diffs'

Missing hash field is treated as unsync

  $ cat > $TESTTMP/mockduit << EOF
  > [[{
  >   "number": 1,
  >   "diff_status_name": "Approved",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3}
  > }]]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  unsync

And finally, the success case

  $ cat > $TESTTMP/mockduit << EOF
  > [[{
  >   "number": 1,
  >   "diff_status_name": "Committed",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"lolwut\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 3}
  > }]]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg log -T '{syncstatus}\n' -r .
  committed
