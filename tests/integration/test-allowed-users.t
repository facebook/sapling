  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd "$TESTTMP/mononoke-config"
  $ cat >> repos/repo/server.toml << EOF
  > [[bookmarks]]
  > regex="A|B|X/Y"
  > allowed_users="^(a|b)$"
  > [[bookmarks]]
  > name="C"
  > allowed_users="^c$"
  > EOF
  $ cd "$TESTTMP"

setup repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ hg debugdrawdag << EOF
  > A B C
  > | | |
  > D E F
  >  \|/
  >   G
  > EOF
  $ hg bookmark A -r A
  $ hg bookmark B -r B
  $ hg bookmark C -r C
  $ hg bookmark G -r G
  $ cd $TESTTMP

setup client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client
  $ cd repo-client
  $ setup_hg_client
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > remotenames =
  > EOF

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo
  $ cd repo-client

push new bookmark
  $ hg up G
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (activating bookmark G)
  $ touch 1 && hg add 1 && hg ci -m 1
  $ MOCK_USERNAME="aslpavel" hgmn push -r . --create --to X/Y
  pushing rev 3dd539927db6 to destination ssh://user@dummy/repo bookmark X/Y
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[push] This user `Some(\"aslpavel\")` is not allowed to move `BookmarkName { bookmark: \"X/Y\" }`",
  remote:     }
  remote:   Caused by:
  remote:     [push] This user `Some("aslpavel")` is not allowed to move `BookmarkName { bookmark: "X/Y" }`
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ MOCK_USERNAME="b" hgmn push -r . --create --to X/Y
  pushing rev 3dd539927db6 to destination ssh://user@dummy/repo bookmark X/Y
  searching for changes
  exporting bookmark X/Y

push updates existing bookmark
  $ hg up A
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark A)
  $ touch 2 && hg add 2 && hg ci -m 2
  $ MOCK_USERNAME="aslapvel" hgmn push -r . --to A
  pushing rev fa8d8af14ee8 to destination ssh://user@dummy/repo bookmark A
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[push] This user `Some(\"aslapvel\")` is not allowed to move `BookmarkName { bookmark: \"A\" }`",
  remote:     }
  remote:   Caused by:
  remote:     [push] This user `Some("aslapvel")` is not allowed to move `BookmarkName { bookmark: "A" }`
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ MOCK_USERNAME="a" hgmn push -r . --to A
  pushing rev fa8d8af14ee8 to destination ssh://user@dummy/repo bookmark A
  searching for changes
  updating bookmark A

enable pushrebase
  $ cat >> .hg/hgrc << EOF
  > pushrebase =
  > EOF

pushrebase
  $ hg up C
  2 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (activating bookmark C)
  $ touch 3 && hg add 3 && hg ci -m 3
  $ MOCK_USERNAME="a" hgmn push -r . --to C
  pushing rev 8f950fe5040c to destination ssh://user@dummy/repo bookmark C
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     [pushrebase] This user `Some("a")` is not allowed to move `BookmarkName { bookmark: "C" }`
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[pushrebase] This user `Some(\"a\")` is not allowed to move `BookmarkName { bookmark: \"C\" }`",
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ MOCK_USERNAME="c" hgmn push -r . --to C
  pushing rev 8f950fe5040c to destination ssh://user@dummy/repo bookmark C
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark C
