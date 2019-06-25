  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITE_PUSH_NAMESPACE_REGEX='^(infinitepush1|infinitepush2)/.+$' setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > infinitepush=
  > commitcloud=
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:(infinitepush1|bad)/.+
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Prepare push
  $ cd repo-push
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new

Valid infinitepush, with pushrebase disabled
  $ hgmn push ssh://user@dummy/repo -r . --to "infinitepush1/123" --create
  pushing to ssh://user@dummy/repo
  searching for changes

Valid infinitepush, with pushrebase enabled
  $ hgmn push ssh://user@dummy/repo -r . --to "infinitepush1/456" --create --config extensions.pushrebase=
  pushing to ssh://user@dummy/repo
  searching for changes

Invalid infinitepush, with pushrebase disabled
  $ hgmn push ssh://user@dummy/repo -r . --to "bad/123" --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Invalid Infinitepush bookmark: bad/123 (Infinitepush bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     While verifying Infinite Push bookmark push
  remote:   Caused by:
  remote:     Invalid Infinitepush bookmark: bad/123 (Infinitepush bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Invalid infinitepush, with pushrebase enabled
  $ hgmn push ssh://user@dummy/repo -r . --to "bad/456" --create --config extensions.pushrebase=
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Invalid Infinitepush bookmark: bad/456 (Infinitepush bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     While verifying Infinite Push bookmark push
  remote:   Caused by:
  remote:     Invalid Infinitepush bookmark: bad/456 (Infinitepush bookmarks must match pattern ^(infinitepush1|infinitepush2)/.+$)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Valid push, with pushrebase disabled
  $ hgmn push ssh://user@dummy/repo -r . --to "plain/123" --create
  pushing rev 47da8b81097c to destination ssh://user@dummy/repo bookmark plain/123
  searching for changes
  exporting bookmark plain/123

Valid push, with pushrebase enabled
  $ hgmn push ssh://user@dummy/repo -r . --to "plain/456" --create --config extensions.pushrebase=
  pushing rev 47da8b81097c to destination ssh://user@dummy/repo bookmark plain/456
  searching for changes
  no changes found
  exporting bookmark plain/456
  [1]

Invalid push, with pushrebase disabled
  $ hgmn push ssh://user@dummy/repo -r . --to "infinitepush2/123" --create
  pushing rev 47da8b81097c to destination ssh://user@dummy/repo bookmark infinitepush2/123
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[push] Only Infinitepush bookmarks are allowed to match pattern ^(infinitepush1|infinitepush2)/.+$",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     [push] Only Infinitepush bookmarks are allowed to match pattern ^(infinitepush1|infinitepush2)/.+$
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Invalid push, with pushrebase enabled
  $ hgmn push ssh://user@dummy/repo -r . --to "infinitepush2/456" --create --config extensions.pushrebase=
  pushing rev 47da8b81097c to destination ssh://user@dummy/repo bookmark infinitepush2/456
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     [push] Only Infinitepush bookmarks are allowed to match pattern ^(infinitepush1|infinitepush2)/.+$
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[push] Only Infinitepush bookmarks are allowed to match pattern ^(infinitepush1|infinitepush2)/.+$",
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Check everything is as expected
  $ cd ..
  $ cd repo-pull
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 47da8b81097c
  $ hg bookmarks --remote
     default/master_bookmark   0:3903775176ed
     default/plain/123         1:47da8b81097c
     default/plain/456         1:47da8b81097c
  $ hgmn bookmarks --list-remote "*"
     infinitepush1/123         47da8b81097c5534f3eb7947a8764dd323cffe3d
     infinitepush1/456         47da8b81097c5534f3eb7947a8764dd323cffe3d
     master_bookmark           3903775176ed42b1458a6281db4a0ccf4d9f287a
     plain/123                 47da8b81097c5534f3eb7947a8764dd323cffe3d
     plain/456                 47da8b81097c5534f3eb7947a8764dd323cffe3d
