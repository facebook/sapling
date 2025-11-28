# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo
  $ cd repo
  $ testtool_drawdag -R repo <<'EOF'
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
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=fa8ba037ceed6e3f11f3bd0d21a866ca4c7a8c721ff13ca7c0b3442e1e4cbb16
  E=2b9b61bcf926e1e354ecb23c529868b1368af2e315b7b8ad720795184764020c
  F=9ba339596c48b2c9b6ed1921e999ecfaef9a03e2f63353e66e642b8f1a12c308
  G=d9ab90dd09369a009cb4f8b913a9a6861c98a86bf4659c712352071503a3d074

Import and start mononoke
  $ cd "$TESTTMP"
  $ mononoke
  $ wait_for_mononoke

setup client repo
  $ hg clone -q mono:repo repo-client --noupdate
  $ cd repo-client

switch to client and enable extension
  $ setconfig extensions.commitcloud=

match with glob pattern
  $ hg book --list-remote test/*
     test/one                  d3b399ca8757acdb81c3681b052eb978db6768d8
     test/three                a5418225e3986ee2c8c49429c777f01469d4c8c7
     test/two                  4a55f53cc855ecee0391508d73e905770b0361e6

match with literal pattern
  $ hg book --list-remote test
  $ hg book --list-remote test/three
     test/three                a5418225e3986ee2c8c49429c777f01469d4c8c7
  $ hg book --list-remote test/t*
     test/three                a5418225e3986ee2c8c49429c777f01469d4c8c7
     test/two                  4a55f53cc855ecee0391508d73e905770b0361e6

match multiple patterns
  $ hg book --list-remote test/one --list-remote test/th*
     test/one                  d3b399ca8757acdb81c3681b052eb978db6768d8
     test/three                a5418225e3986ee2c8c49429c777f01469d4c8c7

match with SQL wildcards doesn't match arbitrary things (should match nothing)
  $ hg book --list-remote t__t/*

match with SQL wildcards does match things with those characters
  $ hg book --list-remote special/__test*
     special/__test__          80521a640a0c8f51dcc128c2658b224d595840ac
