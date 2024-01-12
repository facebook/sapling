#debugruntest-compatible

Setup
  $ configure modern
  $ enable fbcodereview
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
  $ hg prev 2 -q
  [23bffa] add initial
  $ mkcommit d 456
  $ hg go d131c2d7408a
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved


Setup phabricator response
  $ cat > $TESTTMP/mockduit << EOF
  > [
  >   {
  >     "data": {
  >       "phabricator_diff_query": [
  >         {
  >           "results": {
  >             "nodes": [
  >               {
  >                 "number": 123,
  >                 "diff_status_name": "Closed",
  >                 "phabricator_versions": {
  >                   "nodes": [
  >                     {
  >                       "local_commits": [
  >                         {
  >                           "primary_commit": {
  >                             "commit_identifier": "d131c2d7408acf233a4b2db04382005434346421"
  >                           }
  >                         }
  >                       ]
  >                     },
  >                     {
  >                       "local_commits": [
  >                         {
  >                           "primary_commit": {
  >                             "commit_identifier": "a421db7622bf0c454ab19479f166fd4a3a4a41f5"
  >                           }
  >                         }
  >                       ]
  >                     },
  >                     {
  >                       "local_commits": []
  >                     }
  >                   ]
  >                 },
  >                 "phabricator_diff_commit": {
  >                   "nodes": [
  >                     {
  >                       "commit_identifier": "23bffadc9066efde1d8e9f53ee3d5ea9da04ff1b"
  >                     }
  >                   ]
  >                 }
  >               },
  >               {
  >                 "number": 456,
  >                 "diff_status_name": "Abandoned",
  >                 "phabricator_versions": {
  >                   "nodes": [
  >                     {
  >                       "local_commits": [
  >                         {
  >                           "primary_commit": {
  >                             "commit_identifier": "524f3ad51d24452fa525a9053ac8de596a2f047c"
  >                           }
  >                         }
  >                       ]
  >                     }
  >                   ]
  >                 },
  >                 "phabricator_diff_commit": {
  >                   "nodes": []
  >                 }
  >               }
  >             ]
  >           }
  >         }
  >       ]
  >     },
  >     "extensions": {
  >       "is_final": true
  >     }
  >   }
  > ]
  > EOF

Show commit graph
  $ hg log -G -T '{node|short} {desc}\n\n\n'
  o  524f3ad51d24 add d
  │
  │  Differential Revision: https://phabricator.fb.com/D456
  │
  │
  │ @  d131c2d7408a add c
  │ │
  │ │  Differential Revision: https://phabricator.fb.com/D123
  │ │
  │ │
  │ o  a421db7622bf add b
  ├─╯
  │    Differential Revision: https://phabricator.fb.com/D123
  │
  │
  o  23bffadc9066 add initial
  
     Differential Revision: https://phabricator.fb.com/D123

Test that commit hashes matching GraphQL are marked as landed
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (a421db7622bf, d131c2d7408a) as landed as 23bffadc9066
  marking D456 (524f3ad51d24) as abandoned
  marked 2 commits as landed
  marked 1 commit as abandoned
  hiding 3 commits
  (this is a dry-run, nothing was actually done)

Don't hide commits when fbcodereview.hide-landed-commits=false:
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run --config fbcodereview.hide-landed-commits=false
  marking D123 (a421db7622bf, d131c2d7408a) as landed as 23bffadc9066
  marking D456 (524f3ad51d24) as abandoned
  marked 2 commits as landed
  marked 1 commit as abandoned
  (this is a dry-run, nothing was actually done)

Setup amend local commit
  $ mkamend

Test that if the commit hash is changed, then it's no longer marked as landed.
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (a421db7622bf) as landed as 23bffadc9066
  marking D456 (524f3ad51d24) as abandoned
  marked 1 commit as landed
  marked 1 commit as abandoned
  hiding 2 commits
  (this is a dry-run, nothing was actually done)

Test that original behavior of marking local commits as landed even if hashes don't match GraphQL preserves
  $ setconfig pullcreatemarkers.check-local-versions=False
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (3b86866eb2ba, a421db7622bf) as landed as 23bffadc9066
  marking D456 (524f3ad51d24) as abandoned
  marked 2 commits as landed
  marked 1 commit as abandoned
  hiding 3 commits
  (this is a dry-run, nothing was actually done)

Test that if there are non-obsoleted descendants for the abandoned commit, then it's no longer marked as abandoned.
  $ hg goto 524f3ad51d24
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit e 666
  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit hg debugmarklanded --verbose --dry-run
  marking D123 (3b86866eb2ba, a421db7622bf) as landed as 23bffadc9066
  marked 2 commits as landed
  hiding 2 commits
  (this is a dry-run, nothing was actually done)
