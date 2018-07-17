Load extensions

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../hgext/extlib/phabricator/arcconfig.py
  > arcdiff=
  > EOF

Diff with no revision

  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg add foo
  $ hg ci -qm 'No rev'
  $ hg diff --since-last-submit
  abort: local changeset is not associated with a differential revision
  [255]

Fake a diff

  $ echo bleet > foo
  $ hg ci -qm 'Differential Revision: https://phabricator.fb.com/D1'
  $ hg diff --since-last-submit
  abort: no .arcconfig found
  [255]

Prep configuration

  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "cert" : "garbage_cert"}}}' > .arcconfig

Now progressively test the response handling for variations of missing data

  $ cat > $TESTTMP/mockduit << EOF
  > [{}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit
  Error calling graphql: Unexpected graphql response format
  abort: unable to determine previous changeset hash
  [255]

  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Needs Review",
  >   "differential_diffs": {"count": 3},
  >   "is_landing": false,
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit
  abort: unable to determine previous changeset hash
  [255]

  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Needs Review",
  >   "is_landing": false,
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit
  abort: unable to determine previous changeset hash
  [255]

This is the case when the diff is up to date with the current commit;
there is no diff since what was landed.

  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Needs Review",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"2e6531b7dada2a3e5638e136de05f51e94a427f4\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 1},
  >   "is_landing": false,
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit

This is the case when the diff points at our parent commit, we expect to
see the bleet text show up.  There's a fake hash that I've injected into
the commit list returned from our mocked phabricator; it is present to
assert that we order the commits consistently based on the time field.

  $ cat > $TESTTMP/mockduit << EOF
  > [{"data": {"query": [{"results": {"nodes": [{
  >   "number": 1,
  >   "diff_status_name": "Needs Review",
  >   "latest_active_diff": {
  >     "local_commit_info": {
  >       "nodes": [
  >         {"property_value": "{\"lolwut\": {\"time\": 0, \"commit\": \"88dd5a13bf28b99853a24bddfc93d4c44e07c6bd\"}}"}
  >       ]
  >     }
  >   },
  >   "differential_diffs": {"count": 1},
  >   "is_landing": false,
  >   "created_time": 123,
  >   "updated_time": 222
  > }]}}]}}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit --nodates
  diff -r 88dd5a13bf28 -r 2e6531b7dada foo
  --- a/foo
  +++ b/foo
  @@ -0,0 +1,1 @@
  +bleet

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit-2o
  Phabricator rev: 88dd5a13bf28b99853a24bddfc93d4c44e07c6bd
  Local rev: 2e6531b7dada2a3e5638e136de05f51e94a427f4 (.)
  Changed: foo
  | ...
  | +bleet

Make a new commit on top, and then use -r to look at the previous commit
  $ echo other > foo
  $ hg commit -m "Other commmit"
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-submit --nodates -r 2e6531b
  diff -r 88dd5a13bf28 -r 2e6531b7dada foo
  --- a/foo
  +++ b/foo
  @@ -0,0 +1,1 @@
  +bleet
