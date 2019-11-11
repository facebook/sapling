  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ REPOTYPE="blob:files"
  $ REPOID=0 REPONAME=large-mon setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=small-mon setup_common_config $REPOTYPE
  $ cat >> "$TESTTMP/mononoke-config/common/commitsyncmap.toml" <<EOF
  > [megarepo_test]
  > large_repo_id = 0
  > common_pushrebase_bookmarks = ["master_bookmark"]
  >   [[megarepo_test.small_repos]]
  >   repoid = 1
  >   bookmark_prefix = "bookprefix/"
  >   default_action = "prepend_prefix"
  >   default_prefix = "smallrepofolder"
  >   direction = "large_to_small"
  >      [megarepo_test.small_repos.map]
  > EOF

Verification function
  $ function verify_wc() {
  >   local large_repo_commit
  >   large_repo_commit="$1"
  >   "$MONONOKE_ADMIN" "${CACHING_ARGS[@]}" --log-level ERROR --mononoke-config-path "$TESTTMP"/mononoke-config --source-repo-id="$REPOIDLARGE" --target-repo-id="$REPOIDSMALL" crossrepo verify-wc $large_repo_commit
  > }

setup hg server repos
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add -q "$1"; }
  $ function create_first_post_move_commit {
  > echo 1 > "$1/filetoremove" && hg add "$1/filetoremove" && hg ci -m 'first post-move commit'
  > hg revert -r .^ "$1/filetoremove"
  > }

  $ cd $TESTTMP
  $ hginit_treemanifest small-hg-srv
  $ cd small-hg-srv
  $ echo 1 > file.txt
  $ hg addremove -q && hg ci -q -m 'pre-move commit'

  $ cd ..
  $ cp -r small-hg-srv large-hg-srv
  $ cd large-hg-srv
  $ mkdir smallrepofolder
  $ hg mv file.txt smallrepofolder/file.txt
  $ hg ci -m 'move commit'
  $ create_first_post_move_commit smallrepofolder
  $ hg book -r . master_bookmark

  $ cd ..
  $ cd small-hg-srv
  $ create_first_post_move_commit .
  $ hg book -r . master_bookmark

blobimport hg servers repos into Mononoke repos
  $ cd $TESTTMP
  $ REPOIDLARGE=0
  $ REPOIDSMALL=1
  $ REPOID="$REPOIDLARGE" blobimport large-hg-srv/.hg large-mon
  $ REPOID="$REPOIDSMALL" blobimport small-hg-srv/.hg small-mon

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

  $ init_client small-hg-srv small-hg-client
  $ cd "$TESTTMP"
  $ init_client large-hg-srv large-hg-client

Setup helpers
  $ log() {
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }

  $ LARGE_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDLARGE master_bookmark)
  $ SMALL_MASTER_BONSAI=$(get_bonsai_bookmark $REPOIDSMALL master_bookmark)

start mononoke server
  $ mononoke
  $ wait_for_mononoke

Make sure mapping is set up and we know what we don't have to sync initial entries
  $ add_synced_commit_mapping_entry $REPOIDSMALL $SMALL_MASTER_BONSAI $REPOIDLARGE $LARGE_MASTER_BONSAI
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "INSERT INTO mutable_counters (repo_id, name, value) VALUES ($REPOIDSMALL, 'backsync_from_$REPOIDLARGE', 2)";

Normal pushrebase with one commit
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
- compare the working copies
  $ verify_wc master_bookmark

Bookmark-only pushrebase (Create a new bookmark, do not push commits)
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn push -r master_bookmark^ --to master_bookmark_2 --create | grep exporting
  exporting bookmark master_bookmark_2
  $ hg book --all
  no bookmarks set
     default/master_bookmark   2:ce81c7d38286
     default/master_bookmark_2 1:11f848659bfc
-- this is not a `common_pushrebase_bookmark`, so should be prefixed
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg book --all
  no bookmarks set
     default/bookprefix/master_bookmark_2 2:bfcfb674663c
     default/master_bookmark   3:819e91b238b7
- compare the working copies
  $ verify_wc bookprefix/master_bookmark_2

Delete a bookmark
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn push -d master_bookmark_2 | grep deleting
  deleting remote bookmark master_bookmark_2
  $ hg book --all
  no bookmarks set
     default/master_bookmark   2:ce81c7d38286
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg book --all
  no bookmarks set
     default/master_bookmark   3:819e91b238b7

Normal pushrebase with many commits
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ createfile 4 && hg ci -qm "Aeneas was a lively fellow"
  $ createfile 5 && hg ci -qm "Lusty as any Cossack blade"
  $ createfile 6 && hg ci -qm "In every kind of mischief mellow"
  $ createfile 7 && hg ci -qm "The staunchest tramp to ply his trade"

  $ REPONAME=small-mon hgmn push --to master_bookmark
  pushing rev beb30dc3a35c to destination ssh://user@dummy/small-mon bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ log -r master_bookmark
  @  The staunchest tramp to ply his trade [public;rev=6;beb30dc3a35c] default/master_bookmark
  |
  ~
-- this should also be present in a large repo, once we pull:
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  The staunchest tramp to ply his trade [public;rev=7;34c34be6efde] default/master_bookmark
  |
  ~
  $ verify_wc master_bookmark

Pushrebase, which deletes and removes files
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg rm 4 -q
  $ hg mv 5 5.renamed -q
  $ hg cp 6 subdir/6.copy -q
  $ REPONAME=small-mon hgmn ci -m "Moves, renames and copies"
  $ REPONAME=small-mon hgmn push --to master_bookmark | grep updating
  updating bookmark master_bookmark
  $ log -r master_bookmark
  @  Moves, renames and copies [public;rev=7;b888ee4f19b5] default/master_bookmark
  |
  ~
-- this should also be present in a large repo, once we pull:
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  Moves, renames and copies [public;rev=8;b4e3e504160c] default/master_bookmark
  |
  ~
  $ verify_wc master_bookmark

Pushrebase, which replaces a directory with a file
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg rm subdir
  removing subdir/6.copy
  $ createfile subdir && hg ci -qm "Replace a directory with a file"
  $ REPONAME=small-mon hgmn push --to master_bookmark | grep updating
  updating bookmark master_bookmark
  $ log -r master_bookmark
  @  Replace a directory with a file [public;rev=8;e72ee383159a] default/master_bookmark
  |
  ~
-- this should also be present in a large repo, once we pull
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r master_bookmark
  o  Replace a directory with a file [public;rev=9;6ac00e7afd93] default/master_bookmark
  |
  ~
  $ verify_wc master_bookmark
