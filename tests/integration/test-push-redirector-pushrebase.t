  $ . "${TEST_FIXTURES}/library.sh"
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

Pushrebase, which copies and removes files
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

Normal pushrebase to a prefixed bookmark
-- push to create a second bookmark
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark^
  $ createfile epicfail && hg ci -qm "The epicness of this fail is great"
  $ REPONAME=small-mon hgmn push --to master_bookmark_2 --create -q
  $ log -r master_bookmark_2
  @  The epicness of this fail is great [public;rev=9;8d22dc8b8a89] default/master_bookmark_2
  |
  ~
-- this should also be present in a large repo, once we pull
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r bookprefix/master_bookmark_2
  o  The epicness of this fail is great [public;rev=10;030470259cb4] default/bookprefix/master_bookmark_2
  |
  ~
  $ verify_wc bookprefix/master_bookmark_2
-- push to update a second bookmark
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark_2
  $ echo "more epicness" >> epicfail && hg ci -m "The epicness of this fail is greater"
  $ REPONAME=small-mon hgmn push --to master_bookmark_2 | grep updating
  updating bookmark master_bookmark_2
  $ log -r master_bookmark_2
  @  The epicness of this fail is greater [public;rev=10;bd5577e4b538] default/master_bookmark_2
  |
  ~
-- this should also be present in a large repo, once we pull
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ log -r bookprefix/master_bookmark_2
  o  The epicness of this fail is greater [public;rev=11;ccbb367ae93a] default/bookprefix/master_bookmark_2
  |
  ~
  $ verify_wc bookprefix/master_bookmark_2

Pushrebase with a rename between a shifted and a non-shifted behavior
-- let's create a file in a non-shifted directory
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ createfile non_path_shifting/filetomove && createfile filetonotmove
  $ hg ci -qm "But since it is for you, I vow To slap Aeneas down to hell"
  $ REPONAME=small-mon hgmn push --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- let's sync it, make sure everything is fine
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ ls non_path_shifting
  filetomove
  $ verify_wc master_bookmark

-- let's now move this file to a shifted directory
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg mv non_path_shifting/filetomove filetomove
  $ hg ci -qm "I shall delay no longer now But knock him for a fare-you-well."
  $ REPONAME=small-mon hgmn push --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- let's also sync it to the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ ls non_path_shifting/filetomove
  ls: cannot access non_path_shifting/filetomove: No such file or directory
  [2]
  $ ls smallrepofolder/filetomove
  smallrepofolder/filetomove
  $ verify_wc master_bookmark

-- let's now move this file back
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg mv filetomove non_path_shifting/filetomove
  $ hg ci -qm "Now Dido was in such great sorrow All day she neither drank nor ate"
  $ REPONAME=small-mon hgmn push --to master_bookmark | grep updating
  updating bookmark master_bookmark
-- let's also sync it to the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark
  $ ls non_path_shifting
  filetomove
  $ ls smallrepofolder/filetomove
  ls: cannot access smallrepofolder/filetomove: No such file or directory
  [2]
  $ verify_wc master_bookmark
