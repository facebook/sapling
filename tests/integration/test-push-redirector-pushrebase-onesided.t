  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob:files"
  $ REPOID=0 REPONAME=large-mon setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=small-mon-1 setup_common_config $REPOTYPE
  $ REPOID=2 REPONAME=small-mon-2 setup_common_config $REPOTYPE
  $ cat >> "$TESTTMP/mononoke-config/common/commitsyncmap.toml" <<EOF
  > [megarepo_test]
  > large_repo_id = 0
  > common_pushrebase_bookmarks = ["master_bookmark"]
  >   [[megarepo_test.small_repos]]
  >   repoid = 1
  >   bookmark_prefix = "bookprefix1/"
  >   default_action = "prepend_prefix"
  >   default_prefix = "smallrepofolder1"
  >   direction = "large_to_small"
  >      [megarepo_test.small_repos.map]
  >      "special"="specialsmallrepofolder1"
  >   [[megarepo_test.small_repos]]
  >   repoid = 2
  >   bookmark_prefix = "bookprefix2/"
  >   default_action = "prepend_prefix"
  >   default_prefix = "smallrepofolder2"
  >   direction = "small_to_large"
  >      [megarepo_test.small_repos.map]
  >      "special"="specialsmallrepofolder2"
  > EOF

setup configerator configs
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

Verification function
  $ function verify_wc() {
  >   local large_repo_commit
  >   large_repo_commit="$1"
  >   "$MONONOKE_ADMIN" "${CACHING_ARGS[@]}" --log-level ERROR --mononoke-config-path "$TESTTMP"/mononoke-config --source-repo-id="$REPOIDLARGE" --target-repo-id="$REPOIDSMALL1" crossrepo verify-wc $large_repo_commit
  > }

setup hg server repos
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function create_first_post_move_commit {
  >   echo 1 > "$1/filetoremove" && hg add "$1/filetoremove" && hg ci -m 'first post-move commit'
  >   hg revert -r .^ "$1/filetoremove"
  > }

  $ cd $TESTTMP
  $ hginit_treemanifest small-hg-srv-1
  $ hginit_treemanifest small-hg-srv-2
  $ cd "$TESTTMP/small-hg-srv-1"
  $ echo 1 > file.txt
  $ hg addremove -q && hg ci -q -m 'pre-move commit 1'
  $ cd "$TESTTMP/small-hg-srv-2"
  $ echo 2 > file.txt
  $ hg addremove -q && hg ci -q -m 'pre-move commit 2'

  $ cd "$TESTTMP"
  $ cp -r small-hg-srv-1 large-hg-srv
  $ cd large-hg-srv
  $ mkdir smallrepofolder1
  $ hg mv file.txt smallrepofolder1/file.txt
  $ hg ci -m 'move commit'
  $ mkdir smallrepofolder2
  $ echo 2 > smallrepofolder2/file.txt
  $ hg addremove -q
  $ hg ci -m "move commit for repo 2"
  $ create_first_post_move_commit smallrepofolder1
  $ hg book -r . master_bookmark

  $ cd "$TESTTMP/small-hg-srv-1"
  $ create_first_post_move_commit .
  $ hg book -r . master_bookmark

  $ cd "$TESTTMP/small-hg-srv-2"
  $ hg book -r . master_bookmark

blobimport hg servers repos into Mononoke repos
  $ cd $TESTTMP
  $ REPOIDLARGE=0
  $ REPOIDSMALL1=1
  $ REPOIDSMALL2=2
  $ REPOID="$REPOIDLARGE" blobimport large-hg-srv/.hg large-mon
  $ REPOID="$REPOIDSMALL1" blobimport small-hg-srv-1/.hg small-mon-1
  $ REPOID="$REPOIDSMALL2" blobimport small-hg-srv-2/.hg small-mon-2

setup hg client repos
  $ function init_client() {
  > cd "$TESTTMP"
  > hgclone_treemanifest ssh://user@dummy/"$1" "$2" --noupdate --config extensions.remotenames=
  > cd "$TESTTMP/$2"
  > cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  > }

  $ init_client small-hg-srv-1 small-hg-client-1
  $ init_client small-hg-srv-2 small-hg-client-2
  $ cd "$TESTTMP"
  $ init_client large-hg-srv large-hg-client

Setup helpers
  $ log() {
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }

  $ LARGE_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDLARGE master_bookmark)
  $ SMALL1_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDSMALL1 master_bookmark)
  $ SMALL2_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDSMALL2 master_bookmark)

start mononoke server
  $ mononoke --local-configerator-path="$TESTTMP/configerator"
  $ wait_for_mononoke

Make sure mapping is set up and we know what we don't have to sync initial entries
  $ add_synced_commit_mapping_entry $REPOIDSMALL1 $SMALL1_MASTER_BONSAI $REPOIDLARGE $LARGE_MASTER_BONSAI
  $ add_synced_commit_mapping_entry $REPOIDSMALL2 $SMALL2_MASTER_BONSAI $REPOIDLARGE $LARGE_MASTER_BONSAI
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDSMALL1, 'backsync_from_$REPOIDLARGE', 2)";

Normal pushrebase with one commit
  $ cd "$TESTTMP/small-hg-client-1"
  $ REPONAME=small-mon-1 hgmn up -q master_bookmark
  $ echo 2 > 2 && hg addremove -q && hg ci -q -m newcommit
  $ REPONAME=small-mon-1 hgmn push -r . --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- newcommit was correctly pushed to master_bookmark
  $ log -r master_bookmark
  @  newcommit [public;rev=2;6989db12d1e5] default/master_bookmark
  |
  ~

-- newcommit is also present in the large repo (after a pull)
  $ cd "$TESTTMP"/large-hg-client
  $ log -r master_bookmark
  o  first post-move commit [public;rev=3;bca7e9574548] default/master_bookmark
  |
  ~
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  newcommit [public;rev=4;7c9a729ceb57] default/master_bookmark
  |
  ~
- compare the working copies
  $ verify_wc master_bookmark

At the same time, the tailed repo gets new commits
  $ cd "$TESTTMP/small-hg-client-2"
  $ REPONAME=small-mon-2 hgmn up -q master_bookmark
  $ createfile file2_1
  $ hg ci -qm "Post-merge commit 1"
  $ REPONAME=small-mon-2 hgmn push --to master_bookmark -q
-- tailer puts this commit into a large repo
  $ mononoke_x_repo_sync_once $REPOIDSMALL2 $REPOIDLARGE master_bookmark once --commit master_bookmark 2>&1 | grep "synced as"
  * public changeset 46d7f49c05a72a305692183a11274a0fbbdc4f8a4b53ca759fb3d257ba54184e synced as 3a9ffb4771519f86b79729a543da084c6a70ff385933aed540e2112a049a0697 * (glob)

Force pushrebase should fail, because it pushes to a shared bookmark
  $ cd "$TESTTMP/small-hg-client-1"
  $ REPONAME=small-mon-1 hgmn up -q master_bookmark^
  $ echo 3 > 3 && hg add 3 && hg ci -q -m "non-forward move"
  $ REPONAME=small-mon-1 hgmn push --to master_bookmark --force --pushvar NON_FAST_FORWARD=true | grep updating
  remote: Command failed
  remote:   Error:
  remote:     cannot force pushrebase to shared bookmark master_bookmark
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "cannot force pushrebase to shared bookmark master_bookmark",
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [1]

Non-shared bookmark should work
  $ REPONAME=small-mon-1 hgmn push --to master_bookmark_non_fast_forward --force --create -q
-- it should also be present in a large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r bookprefix1/master_bookmark_non_fast_forward
  o  non-forward move [public;rev=5;6b6a308437bb] default/bookprefix1/master_bookmark_non_fast_forward
  |
  ~

Bookmark-only pushrebase (Create a new bookmark, do not push commits)
  $ cd "$TESTTMP/small-hg-client-1"
  $ REPONAME=small-mon-1 hgmn push -r master_bookmark^ --to master_bookmark_2 --create | grep exporting
  exporting bookmark master_bookmark_2
  $ hg book --all
  no bookmarks set
     default/master_bookmark   2:6989db12d1e5
     default/master_bookmark_2 1:680aaf36d7a2
     default/master_bookmark_non_fast_forward 3:161addaa86c7
-- this is not a `common_pushrebase_bookmark`, so should be prefixed
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg book --all
  no bookmarks set
     default/bookprefix1/master_bookmark_2 3:bca7e9574548
     default/bookprefix1/master_bookmark_non_fast_forward 5:6b6a308437bb
     default/master_bookmark   6:bf8e8d65212d
- compare the working copies
  $ verify_wc bookprefix1/master_bookmark_2

Delete a bookmark
  $ cd "$TESTTMP/small-hg-client-1"
  $ REPONAME=small-mon-1 hgmn push -d master_bookmark_2 | grep deleting
  deleting remote bookmark master_bookmark_2
  $ hg book --all
  no bookmarks set
     default/master_bookmark   2:6989db12d1e5
     default/master_bookmark_non_fast_forward 3:161addaa86c7
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg book --all
  no bookmarks set
     default/bookprefix1/master_bookmark_non_fast_forward 5:6b6a308437bb
     default/master_bookmark   6:bf8e8d65212d
