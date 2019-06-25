  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ INFINITE_PUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ mkcommit commit1
  $ NODE1="$(hg log -l 1 --template '{node}\n' -r .)"
  $ mkcommit commit2
  $ NODE2="$(hg log -l 1 --template '{node}\n' -r .)"
  $ echo "$NODE1"
  cb9a30b04b9df854f40d21fdac525408f3bd6c78
  $ echo "$NODE2"
  86383633ba7ff1d50a8d2990f0b63d2401110c26

set constants

  $ BOOK_OK="scratch/123"
  $ BOOK_BAD="master"

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage
  $ cd ..
  $ blobimport repo-hg/.hg repo

Create the replaybookmarks table
  $ create_replaybookmarks_table

Run the filler with no work
  $ mononoke_bookmarks_filler --max-iterations 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Processing batch: 0 entries (glob)

Run the filler with valid work (create)
  $ insert_replaybookmarks_entry repo "$BOOK_OK" "$NODE1"
  $ mononoke_bookmarks_filler --max-iterations 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Processing batch: 1 entries (glob)
  * Updating bookmark BookmarkName { bookmark: "scratch/123" }: None -> ChangesetId(Blake2(d2ebff6a6aa240a684a4623afd028afd208d3f81f06f0e525b2fd11eb6ba47ac)) (glob)
  * Outcome: bookmark: BookmarkName { bookmark: "scratch/123" }: success: true (glob)
  $ mononoke_admin bookmarks get "$BOOK_OK"
  * INFO using repo "repo" repoid RepositoryId(0) (glob)
  (HG) cb9a30b04b9df854f40d21fdac525408f3bd6c78

Run the filler with valid work (update)
  $ insert_replaybookmarks_entry repo "$BOOK_OK" "$NODE2"
  $ mononoke_bookmarks_filler --max-iterations 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Processing batch: 1 entries (glob)
  * Updating bookmark BookmarkName { bookmark: "scratch/123" }: Some(ChangesetId(Blake2(d2ebff6a6aa240a684a4623afd028afd208d3f81f06f0e525b2fd11eb6ba47ac))) -> ChangesetId(Blake2(c97399683492face21a2dcc6c422e117ec67365b87ecb53c4152c0052945bdfe)) (glob)
  * Outcome: bookmark: BookmarkName { bookmark: "scratch/123" }: success: true (glob)
  $ mononoke_admin bookmarks get "$BOOK_OK"
  * INFO using repo "repo" repoid RepositoryId(0) (glob)
  (HG) 86383633ba7ff1d50a8d2990f0b63d2401110c26

Run the filler with valid work (bad bookmark)
  $ insert_replaybookmarks_entry repo "$BOOK_BAD" "$NODE2"
  $ mononoke_bookmarks_filler --max-iterations 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Processing batch: 1 entries (glob)
  * Outcome: bookmark: BookmarkName { bookmark: "master" }: success: false (glob)
