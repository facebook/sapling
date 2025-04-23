# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > sparse=
  > EOF

setup repo
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B-C
  > # modify: A "a" "file_content"
  > # modify: B "b" "file_content"
  > # modify: C "c" "file_content"
  > # bookmark: C master_bookmark
  > EOF
  A=06e03b1b8f6dd9f3a5868cb5197ebf3ed8812ed3
  B=f9a27f526038f019dce279a56566b848ade238d2
  C=ebf6f9404607c1addccefe62e8ed8febc2865958

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hgsparse_profile <<EOF
  > [include]
  > foo
  > EOF
  $ hg commit -Aqm 'Add sparse profile'
  $ hg sparse enable .hgsparse_profile
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
