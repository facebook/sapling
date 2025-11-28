# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ POPULATE_GIT_MAPPING=1 setup_common_config

setup repo
  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R repo --no-default-files <<EOF
  > A-B
  > # modify: A "a" "a\n"
  > # modify: B "a" "a\n"
  > # modify: B "b" "b\n"
  > # message: A "a"
  > # message: B "b"
  > # extra: A convert_revision "37b0a167e07f2b84149c918cec818ffeb183dddd"
  > # extra: A hg-git-rename-source "git"
  > # extra: B convert_revision "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  > # extra: B hg-git-rename-source "git"
  > # bookmark: B master_bookmark
  > EOF
  A=ed9644eae39ff1952e43102db42c36faf093e042
  B=795c4a7447011a567dfc4f73e15702962cf801d4

backfill git mapping
  $ echo $A > $TESTTMP/hash_list
  $ echo $B >> $TESTTMP/hash_list
  $ backfill_mapping --git $TESTTMP/hash_list

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd
  $ hg clone -q mono:repo client
  $ cd client
  $ hg up -q "min(all())"

  $ hg paths
  default = mono:repo
  $ hg id -r _gitlookup_git_37b0a167e07f2b84149c918cec818ffeb183dddd mono:repo
  ed9644eae39f
  $ hg id -r _gitlookup_hg_$A mono:repo
  37b0a167e07f
  $ hg id -r _gitlookup_hg_$B mono:repo
  bbbbbbbbbbbb

We have bookmark with the same name which points to d5b0942fd0ec9189debf6915e9505390564e1247
Make sure that git lookup takes preference
  $ hg id -r _gitlookup_git_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb mono:repo
  795c4a744701
