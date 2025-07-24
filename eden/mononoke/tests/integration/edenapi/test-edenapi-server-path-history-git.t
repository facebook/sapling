# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_mononoke_config
  $ setup_common_config
  $ testtool_drawdag -R repo --derive-all << EOF
  > A-B-C
  > # bookmark: C heads/main
  > # modify: A x a_content
  > # modify: B x b_content
  > EOF
  A=60ff696b167645d7e3cb0db73165400ce696aa4786955e11f3cbf7a031ee4b94
  B=f037431cad95f3ea76efe4c1cda16728980dd9b7754ec70ead432d423105c9b1
  C=f7cd32806db5c0c961ec9249abe8a1946933920f704178b033a8a15651a749cb

  $ start_and_wait_for_mononoke_server

  $ A_GIT="$(mononoke_admin convert -R repo -f bonsai -t git --derive $A)"
  $ B_GIT="$(mononoke_admin convert -R repo -f bonsai -t git --derive $B)"
  $ C_GIT="$(mononoke_admin convert -R repo -f bonsai -t git --derive $C)"
  $ echo $A_GIT
  75e999f64908e2d05bea9a46067d24bdedeeb41d
  $ echo $B_GIT
  c7dd666200707e650996f8007fbfd57c7371d348
  $ echo $C_GIT
  c4f2046634d7639ba94a58507cb75270d667b0b0

Query history with slapigit using git sha1 input
  $ hg --config remotefilelog.reponame=repo --config edenapi.url=https://localhost:$MONONOKE_SOCKET/slapigit/ --config edenapi.ignore-capabilities=true debugapi -e path_history -i "'$C_GIT'" -i "['x']" -i None -i "[]"
  [{"path": "x",
    "entries": {"Ok": {"entries": [{"commit": bin("c7dd666200707e650996f8007fbfd57c7371d348")},
                                   {"commit": bin("75e999f64908e2d05bea9a46067d24bdedeeb41d")}],
                       "has_more": False,
                       "next_commits": []}}}]
