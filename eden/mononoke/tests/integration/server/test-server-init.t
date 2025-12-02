# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ REPOID=1 REPONAME=disabled_repo ENABLED=false setup_mononoke_config
  $ cd $TESTTMP
  $ setconfig remotenames.selectivepulldefault=master_bookmark,master_bookmark2

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > a-b-c-newdir-modifyfile-rename
  > # modify: a "a" "\n"
  > # modify: b "a" "\n"
  > # modify: b "b" "\n"
  > # modify: c "a" "\n"
  > # modify: c "b" "\n"
  > # modify: c "c" "content\n"
  > # modify: newdir "a" "\n"
  > # modify: newdir "b" "\n"
  > # modify: newdir "c" "content\n"
  > # modify: newdir "dir/1" "1\n"
  > # modify: newdir "dir/2" "2\n"
  > # modify: modifyfile "a" "\n"
  > # modify: modifyfile "b" "\n"
  > # modify: modifyfile "c" "cc\n"
  > # modify: modifyfile "dir/1" "1\n"
  > # modify: modifyfile "dir/2" "2\n"
  > # copy: rename "dir/rename" "1\n" modifyfile "dir/1"
  > # delete: rename "dir/1"
  > # modify: rename "a" "\n"
  > # modify: rename "b" "\n"
  > # modify: rename "c" "cc\n"
  > # modify: rename "dir/2" "2\n"
  > # message: a "a"
  > # message: b "b"
  > # message: c "c"
  > # message: newdir "new directory"
  > # message: modifyfile "modify file"
  > # message: rename "rename"
  > # bookmark: rename master_bookmark2
  > EOF
  a=6a2ad26f394ca4d270bea9aa4ef57731a99a46057eb790789ff4eee79a7ba5f3
  b=a813a976d95b5674a7ccdf34c815a5a136ba67b9437feeccbefb4387ca3acefb
  c=f1fa7c6ecbeb4497745f10de2fb3c37d424f6ae3a0e204f5ac48aca61ea8ed9a
  modifyfile=f0f10d3a94843d1dcbcd170b4ead2e5da946d16020befc2a1549aa97d0ab32cc
  newdir=62651ea03b71d00067a442c8c11490eaaee62d81bfa4aeacb831be3d2c4abd7d
  rename=c6c8fe2b4788a2c4f851f333cf6bda1854622643db48960b6406eda3116d392e

  $ testtool_drawdag -R repo --no-default-files <<'EOF'
  > A-B-D
  > A-C-D
  > # modify: A "D" "x\n"
  > # modify: B "D" "1\n"
  > # modify: C "D" "2\n"
  > # modify: D "D" "1\n2\n"
  > # message: A "A"
  > # message: B "B"
  > # message: C "C"
  > # message: D "D"
  > # bookmark: D master_bookmark
  > EOF
  A=c28cb6ca9425c818e0542de681d71608239dff255c35f7c00221e442d78606f2
  B=3554e5586aa62f5b9b6ce24ae0a0adcfb91eb253058eea80795b189602abbb4d
  C=722771c3b789544faf7a86626709ddd6fa0c450452cf6480441dfd1661aeddf4
  D=e75cd16fce5a00c2722a6d53bc57641a7bb36560d0978123d93f41fcd1b7a452
  $ cd $TESTTMP

start mononoke

  $ mononoke
  $ wait_for_mononoke

setup repo2
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotefilelog=
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath
  > EOF
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd $TESTTMP
  $ hg debugwireargs mono:disabled_repo one two --three three
  remote: Unknown Repo:
  remote:   Error:
  remote:     Requested repo "disabled_repo" does not exist or is disabled
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg debugwireargs mono:repo one two --three three
  one two three None None

  $ cd repo2
  $ hg pull -q
  $ HG_C_HASH=$(hg log -r 'desc(C) and ancestors(master_bookmark)' -T '{node}')
  $ cd $TESTTMP

Create repo3 to test pull by specific hash
  $ hg clone -q mono:repo repo3 --noupdate
  $ cd repo3
  $ hg up -q "min(all())"
Test a pull of one specific revision by hash
  $ hg pull -r $HG_C_HASH -q
  $ hg log -r $HG_C_HASH -T '{desc}\n'
  C

  $ cd ../repo2

Verify the merge DAG was created correctly
  $ hg log -r 'ancestors(master_bookmark)' --graph -T '{node|short} {desc}'
  o    028746d393f5 D
  ├─╮
  │ o  b0d80666c945 C
  │ │
  o │  40fd45cc2dd2 B
  ├─╯
  o  7459d653bda7 A
   (re)

  $ hg log -r 'ancestors(master_bookmark2)' --graph  -T '{node|short} {desc}'
  o  280cea29404f rename
  │
  o  a1af22172c7e modify file
  │
  o  34ec82955987 new directory
  │
  o  635623e45e72 c
  │
  o  c79659601245 b
  │
  o  859ac8b08f08 a
   (re)
  $ ls
  $ hg up master_bookmark2 -q
  $ ls
  a
  b
  c
  dir
  $ cat c
  cc
  $ hg up master_bookmark2 -q
  $ hg log c -T '{node|short} {desc}\n'
  warning: file log can be slow on large repos - use -f to speed it up
  a1af22172c7e modify file
  635623e45e72 c
  $ cat dir/rename
  1
  $ cat dir/2
  2
  $ hg log dir/rename -f -T '{node|short} {desc}\n'
  280cea29404f rename
  34ec82955987 new directory
  $ hg st --change master_bookmark2 -C
  A dir/rename
    dir/1
  R dir/1

  $ hg up -q master_bookmark

Sort the output because it may be unpredictable because of the merge
  $ hg log D --follow -T '{node|short} {desc}\n' | sort
  028746d393f5 D
  40fd45cc2dd2 B
  7459d653bda7 A
  b0d80666c945 C

Create a new bookmark and try and send it over the wire
Test commented while we have no bookmark support in blobimport or easy method
to create a fileblob bookmark
#  $ cd ../repo
#  $ hg bookmark test-bookmark
#  $ hg bookmarks
#   * test-bookmark             0:3903775176ed
#  $ cd ../repo2
#  $ hg pull mono:repo
#  pulling from ssh://user@dummy/repo
#  searching for changes
#  no changes found
#  adding remote bookmark test-bookmark
#  $ hg bookmarks
#     test-bookmark             0:3903775176ed

Do a clone of the repo
  $ hg clone mono:repo repo-streamclone
  fetching lazy changelog
  populating main commit graph
  updating to tip
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
