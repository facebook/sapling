# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ setup_common_config
  $ cd $TESTTMP

Setup testing repo for mononoke with multiple bookmarks:
  $ testtool_drawdag -R repo --print-hg-hashes << EOF
  > C E G
  > | | |
  > B D F
  >  \|/
  >   A
  > # bookmark: C test/one
  > # bookmark: E test/two
  > # bookmark: G test/three
  > # bookmark: B special/__test__
  > # bookmark: D special/xxtestxx
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8
  D=8aa9560e921df1a2d4503bbfa9875505ef5e65eb
  E=4a55f53cc855ecee0391508d73e905770b0361e6
  F=75e28e690892ad8d7849e877ea16f8196d444c41
  G=a5418225e3986ee2c8c49429c777f01469d4c8c7

Start up SaplingRemoteAPI server.
  $ start_and_wait_for_mononoke_server

Clone repo
  $ hg clone -q mono:repo repo-client
  $ cd repo-client

Test exact bookmark match
  $ hg debugapi -e listbookmarkpatterns -i '["test/one"]'
  {"test/one": "d3b399ca8757acdb81c3681b052eb978db6768d8"}

Test multiple exact bookmarks
  $ hg debugapi -e listbookmarkpatterns -i '["test/one", "test/two"]'
  {"test/one": "d3b399ca8757acdb81c3681b052eb978db6768d8",
   "test/two": "4a55f53cc855ecee0391508d73e905770b0361e6"}

Test glob pattern with wildcard
  $ hg debugapi -e listbookmarkpatterns -i '["test/*"]' --sort
  {"test/one": "d3b399ca8757acdb81c3681b052eb978db6768d8",
   "test/two": "4a55f53cc855ecee0391508d73e905770b0361e6",
   "test/three": "a5418225e3986ee2c8c49429c777f01469d4c8c7"}

Test partial prefix match
  $ hg debugapi -e listbookmarkpatterns -i '["test/t*"]' --sort
  {"test/two": "4a55f53cc855ecee0391508d73e905770b0361e6",
   "test/three": "a5418225e3986ee2c8c49429c777f01469d4c8c7"}

Test non-existent bookmark (should return empty)
  $ hg debugapi -e listbookmarkpatterns -i '["nonexistent"]'
  {}

Test non-matching pattern (should return empty)
  $ hg debugapi -e listbookmarkpatterns -i '["nomatch/*"]'
  {}

Test special characters in bookmark names
  $ hg debugapi -e listbookmarkpatterns -i '["special/*"]' --sort
  {"special/__test__": "80521a640a0c8f51dcc128c2658b224d595840ac",
   "special/xxtestxx": "8aa9560e921df1a2d4503bbfa9875505ef5e65eb"}

Test SQL wildcards don't match arbitrary things (should match nothing)
  $ hg debugapi -e listbookmarkpatterns -i '["t__t/*"]'
  {}

Test SQL wildcards match things with those characters
  $ hg debugapi -e listbookmarkpatterns -i '["special/__test*"]'
  {"special/__test__": "80521a640a0c8f51dcc128c2658b224d595840ac"}

Test multiple patterns in single call
  $ hg debugapi -e listbookmarkpatterns -i '["test/one", "special/*"]' --sort
  {"test/one": "d3b399ca8757acdb81c3681b052eb978db6768d8",
   "special/__test__": "80521a640a0c8f51dcc128c2658b224d595840ac",
   "special/xxtestxx": "8aa9560e921df1a2d4503bbfa9875505ef5e65eb"}
