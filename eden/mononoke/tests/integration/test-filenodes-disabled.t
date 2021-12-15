# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ setup_common_config
  $ cd $TESTTMP
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "filenodes_disabled": true
  >   }
  > }
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch base
  $ hg commit -Aqm base
  $ tglogp
  @  df4f53cec30a draft 'base'
  

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
  $ wait_for_mononoke


Push a a few commits
  $ cd "$TESTTMP/repo-push"
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hgmn up master_bookmark -q
  $ echo 1 > 1
  $ echo 2 > 2
  $ echo 3 > 3
  $ mkdir dir
  $ echo file > dir/file
  $ hg -q addremove
  $ hg ci -m 'first commit'

  $ echo 1a > 1
  $ echo 2a > 2
  $ hg rm 3
  $ echo newfile > newfile
  $ hg -q addremove
  $ hg ci -m 'second commit'

  $ hgmn push -r . --to master_bookmark -q

Now pull and update to them
  $ cd "$TESTTMP/repo-pull"
  $ setup_hg_client
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hgmn up -q master_bookmark
  $ ls
  1
  2
  base
  dir
  newfile
