#chg-compatible
#debugruntest-compatible

Setup
  $ configure modern
  $ enable pullcreatemarkers
  $ setconfig pullcreatemarkers.check-local-versions=True

Configure arc...
  $ echo '{}' > .arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > .arcconfig

Test that hg pull creates mutation records for landed diffs
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    [ -z "$2" ] || echo "Differential Revision: https://phabricator.fb.com/D$2" >> msg
  >    hg ci -l msg
  > }
  $ mkamend() {
  >    hg log -r. -T'{desc}\n' > msg
  >    echo "Reviewed By: someone" >> msg
  >    hg ci --amend -l msg
  > }

Set up repository with 1 public and 2 local commits
  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ mkcommit initial 123 # 123 is the phabricator rev number (see function above)
  $ hg debugmakepublic 'desc(init)'
  $ mkcommit b 123
  $ mkcommit c 123


Setup phabricator response
  $ cat > $TESTTMP/mockduit << EOF
  > [{
  >   "data": {
  >     "phabricator_diff_query": [
  >       {
  >         "results": {
  >           "nodes": [
  >             {
  >               "number": 123,
  >               "phabricator_versions": {
  >                 "nodes": [
  >                   {"local_commits": [{"primary_commit": {"commit_identifier": "d131c2d7408acf233a4b2db04382005434346421"}}]},
  >                   {"local_commits": [{"primary_commit": {"commit_identifier": "a421db7622bf0c454ab19479f166fd4a3a4a41f5"}}]},
  >                   {"local_commits": []}
  >                 ]
  >               },
  >               "phabricator_diff_commit": {
  >                 "nodes": [
  >                   {"commit_identifier": "23bffadc9066efde1d8e9f53ee3d5ea9da04ff1b"}
  >                 ]
  >               }
  >             }
  >           ]
  >         }
  >       }
  >     ]
  >   },
  >   "extensions": {
  >     "is_final": true
  >   }
  > }]
  > EOF

Test that commit hashes matching GraphQL are marked as landed
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (a421db7622bf, d131c2d7408a) as landed as 23bffadc9066
  marked 2 commits as landed
  (this is a dry-run, nothing was actually done)

Setup amend local commit
  $ mkamend

Test that if the commit hash is changed, then it's no longer marked as landed.
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (a421db7622bf) as landed as 23bffadc9066
  marked 1 commit as landed
  (this is a dry-run, nothing was actually done)

Test that original behavior of marking local commits as landed even if hashes don't match GraphQL preserves
  $ setconfig pullcreatemarkers.check-local-versions=False
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (3b86866eb2ba, a421db7622bf) as landed as 23bffadc9066
  marked 2 commits as landed
  (this is a dry-run, nothing was actually done)
