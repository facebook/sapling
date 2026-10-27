# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

Record which wireproto commands read-only client operations send to Mononoke.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

setup repo
  $ cd $TESTTMP
  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A-B
  > # modify: A "a" "a\n"
  > # modify: B "b" "b\n"
  > # bookmark: B master_bookmark
  > # message: A "a"
  > # message: B "b"
  > EOF
  A=9c7bec57ce44e5380b77562bf4bf9f41df23585d
  B=4bbe7bd603b4908c5e5bc14bc8b37061ef7c7c4e

start mononoke
  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo client
  $ hg clone -q mono:repo pusher

`wire CMD...` runs CMD with --debug and prints only the wireproto commands it sends.
  $ wire() {
  >   "$@" --debug 2>&1 | grep -E '^sending [a-z_]+ command$'
  > }

push
  $ cd pusher
  $ echo c > c
  $ hg commit -qAm c
  $ wire hg push -r . --to other_bookmark --create
  sending hello command
  sending unbundle command
  $ hg log -r . -T '{node}\n'
  19599c85a6f2ea3ba4b6e96a92c3fd69762390a2

pull
  $ cd ../client
  $ wire hg pull -B other_bookmark
  sending hello command
  $ hg log -r other_bookmark -T '{node}\n'
  19599c85a6f2ea3ba4b6e96a92c3fd69762390a2

list remote bookmarks
  $ hg bookmarks --remote
     remote/master_bookmark           4bbe7bd603b4908c5e5bc14bc8b37061ef7c7c4e
     remote/other_bookmark            19599c85a6f2ea3ba4b6e96a92c3fd69762390a2
  $ wire hg bookmarks --remote
  sending hello command
  $ wire hg bookmarks --remote --config experimental.slapi-listkeys-bookmarks=false
  sending hello command
  sending listkeys command

identify remote revisions by bookmark, full hash and hash prefix
  $ hg id -r master_bookmark mono:repo
  4bbe7bd603b4
  $ wire hg id -r master_bookmark mono:repo
  sending hello command
  sending lookup command
  $ hg id -r $A mono:repo
  9c7bec57ce44
  $ wire hg id -r $A mono:repo
  sending hello command
  sending lookup command
  $ hg id -r ${A:0:8} mono:repo
  9c7bec57ce44
  $ wire hg id -r ${A:0:8} mono:repo
  sending hello command
  sending lookup command
  $ hg id -r unknown_bookmark mono:repo
  abort: unknown_bookmark not found!
  [255]
  $ wire hg id -r unknown_bookmark mono:repo
  sending hello command
  sending lookup command

remote() revset
  $ hg log -r "remote(master_bookmark)" -T '{node}\n'
  4bbe7bd603b4908c5e5bc14bc8b37061ef7c7c4e
  $ wire hg log -r "remote(master_bookmark)" -T '{node}\n'
  sending hello command
  sending lookup command

known
  $ hg debugknown mono:repo $A 1111111111111111111111111111111111111111
  10
  $ wire hg debugknown mono:repo $A 1111111111111111111111111111111111111111
  sending hello command
  sending known command

clone with --updaterev
  $ cd $TESTTMP
  $ hg clone -q --updaterev ${A:0:8} mono:repo client2
  $ hg -R client2 log -r . -T '{node}\n'
  9c7bec57ce44e5380b77562bf4bf9f41df23585d
  $ wire hg clone -q --updaterev ${A:0:8} mono:repo client3
  sending hello command
  sending lookup command
