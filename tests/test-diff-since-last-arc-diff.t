Load extensions

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > arcconfig=$TESTDIR/../phabricator/arcconfig.py
  > arcdiff=$TESTDIR/../hgext3rd/arcdiff.py
  > EOF

Diff with no revision

  $ hg init repo
  $ cd repo
  $ touch foo
  $ hg add foo
  $ hg ci -qm 'No rev'
  $ hg diff --since-last-arc-diff
  abort: local commit is not associated with a differential revision
  [255]

Fake a diff

  $ echo bleet > foo
  $ hg ci -qm 'Differential Revision: https://phabricator.fb.com/D1'
  $ hg diff --since-last-arc-diff
  abort: no .arcconfig found
  [255]

Prep configuration

  $ echo '{}' > .arcconfig
  $ echo '{}' > .arcrc

Now progressively test the response handling for variations of missing data

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": null}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff
  abort: unable to determine previous commit hash
  [255]

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": [{"diffs": []}]}]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff
  abort: unable to determine previous commit hash
  [255]

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": [{"diffs": [1]}]},
  >  {"cmd": ["differential.getdiffproperties", {
  >              "diff_id": 1,
  >              "names": ["local:commits"]}],
  >   "result": []
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff
  abort: unable to determine previous commit hash
  [255]

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": [{"diffs": [1]}]},
  >  {"cmd": ["differential.getdiffproperties", {
  >              "diff_id": 1,
  >              "names": ["local:commits"]}],
  >   "result": {"local:commits": []}
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff
  abort: unable to determine previous commit hash
  [255]

This is the case when the diff is up to date with the current commit;
there is no diff since what was landed.

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": [{"diffs": [1]}]},
  >  {"cmd": ["differential.getdiffproperties", {
  >              "diff_id": 1,
  >               "names": ["local:commits"]}],
  >   "result": {"local:commits": {
  >                  "2e6531b7dada2a3e5638e136de05f51e94a427f4": {
  >                      "commit": "2e6531b7dada2a3e5638e136de05f51e94a427f4",
  >                      "time": 0}}}
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff

This is the case when the diff points at our parent commit, we expect to
see the bleet text show up.  There's a fake hash that I've injected into
the commit list returned from our mocked phabricator; it is present to
assert that we order the commits consistently based on the time field.

  $ cat > $TESTTMP/mockduit << EOF
  > [{"cmd": ["differential.query", {"ids": ["1"]}], "result": [{"diffs": [1]}]},
  >  {"cmd": ["differential.getdiffproperties", {
  >              "diff_id": 1,
  >              "names": ["local:commits"]}],
  >   "result": {"local:commits": {
  >                  "88dd5a13bf28b99853a24bddfc93d4c44e07c6bd": {
  >                      "commit": "88dd5a13bf28b99853a24bddfc93d4c44e07c6bd",
  >                      "time": 10},
  >                  "fakehashtotestsorting": {
  >                      "commit": "fakehashtotestsorting",
  >                      "time": 5}}}
  > }]
  > EOF
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg diff --since-last-arc-diff --nodates
  diff -r 88dd5a13bf28 foo
  --- a/foo
  +++ b/foo
  @@ -0,0 +1,1 @@
  +bleet
