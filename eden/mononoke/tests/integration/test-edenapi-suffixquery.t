# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ setconfig experimental.edenapi-suffixquery=true

  $ start_and_wait_for_mononoke_server
  $ hgmn_init repo
  $ cd repo
  $ drawdag << EOS
  > D # D/bar = zero\nuno\ntwo\n
  > |
  > C # C/bar = zero\none\ntwo\n (renamed from foo)
  > |
  > B # B/foo = one\ntwo\n
  > |
  > A # A/foo = one\n
  > EOS

Test suffix query output:
  $ hgedenapi debugapi -e suffix_query -i "{'Hg': 'e9ace545f925b6f62ae34087895fdc950d168e5f'}" -i "['.txt']"
  [{"file_path": ""},
   {"file_path": ""}]
