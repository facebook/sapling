# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ DISALLOW_NON_PUSHREBASE=1 ALLOW_CASEFOLDING=1 BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ hg up -q master_bookmark

Create commit which only differs in case
  $ touch foo.txt Foo.txt
  $ hg ci -Aqm commit1

Push the commit, showing the flag has worked
  $ hg push -q -r . --to master_bookmark || (sed -nr -e 's/, uuid:.*//' -e 's/^.*\] (Caused by.*)/\1/p' "$TESTTMP"/mononoke.out; exit 1)
