  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setup_configerator_configs
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

  $ init_large_small_repo --local-configerator-path="$TESTTMP/configerator"
  Setting up hg server repos
  Blobimporting them
  Starting Mononoke server
  Adding synced mapping entry

-- normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon hgmn push -r . --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- newcommit was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=2;ce81c7d38286] default/master_bookmark
  |
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  o  first post-move commit [public;rev=2;bfcfb674663c] default/master_bookmark
  |
  ~
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=3;819e91b238b7] default/master_bookmark
  |
  ~

-- Mononoke hg sync job: the commit is now present in the small hg repo
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDSMALL" mononoke_hg_sync small-hg-srv 2 2>&1 | grep "successful sync"
  * successful sync of entries [4] (glob)
  $ cd small-hg-srv
  $ log -r :
  o  newcommit [public;rev=2;ce81c7d38286]
  |
  @  first post-move commit [public;rev=1;11f848659bfc]
  |
  o  pre-move commit [public;rev=0;fc7ae591de0e]
  
  $ hg show master_bookmark
  changeset:   2:ce81c7d38286
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       2
  description:
  newcommit
  
  
  diff -r 11f848659bfc -r ce81c7d38286 2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/2	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +2
  
