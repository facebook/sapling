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

Push a merge from a large repo
  $ cd "$TESTTMP/large-hg-client"
  $ mkdir smallrepofolder/
  $ echo 1 > smallrepofolder/newrepo
  $ hg addremove -q
  $ hg ci -m "newrepo"
  $ NODE="$(hg log -r . -T '{node}')"
  $ REPONAME=large-mon hgmn up -q master_bookmark^
  $ hg merge -r "$NODE" -q
  $ hg ci -m 'merge commit from large repo'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

Push a merge that will not add any new files to the small repo
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkdir someotherrepo/
  $ echo 1 > someotherrepo/newrepo
  $ hg addremove -q
  $ hg ci -m "second newrepo"
  $ NODE="$(hg log -r . -T '{node}')"
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ hg merge -r "$NODE" -q
  $ hg ci -m 'merge commit no new files'
  $ REPONAME=large-mon hgmn push -r . --to master_bookmark -q

Backsync to a small repo
  $ backsync_large_to_small 2>&1 | grep "syncing bookmark"
  * syncing bookmark master_bookmark to * (glob)
  * syncing bookmark master_bookmark to * (glob)

Pull from a small repo. Check that both merges are synced
although the second one became non-merge commit
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ log -r :
  o  merge commit no new files [public;rev=4;534a740cd266] default/master_bookmark
  |
  o    merge commit from large repo [public;rev=3;246c2e616e99]
  |\
  | o  newrepo [public;rev=2;64d197011743]
  |
  o  first post-move commit [public;rev=1;11f848659bfc]
  |
  o  pre-move commit [public;rev=0;fc7ae591de0e]
  $
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ REPONAME=small-mon hgmn show master_bookmark
  changeset:   4:534a740cd266
  tag:         tip
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  merge commit no new files
  
  
  

Make sure we have directory from the first move, but not from the second
  $ ls
  file.txt
  filetoremove
  newrepo
