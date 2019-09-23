  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
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


Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push ssh://user@dummy/repo -r . --to "scratch/123"
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Infinitepush is not enabled on this server. Contact Source Control @ FB.",
  remote:     }
  remote:   Caused by:
  remote:     While resolving Changegroup
  remote:   Caused by:
  remote:     Infinitepush is not enabled on this server. Contact Source Control @ FB.
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ tglogp
  @  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Bookmark push should have been ignored
  $ sqlite3 "$TESTTMP/monsql/bookmarks" 'SELECT name, hg_kind, HEX(changeset_id) FROM bookmarks;'
  master_bookmark|pull_default|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793

Commit should have been rejected
  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hgmn pull -r 47da8b81097c5534f3eb7947a8764dd323cffe3d
  pulling from ssh://user@dummy/repo
  abort: 47da8b81097c5534f3eb7947a8764dd323cffe3d not found!
  [255]
