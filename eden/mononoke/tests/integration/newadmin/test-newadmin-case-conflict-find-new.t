# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ mononoke_testtool drawdag -R repo <<'EOF'
  > A-B-C-D
  > # bookmark: C main
  > # modify: B "eden/mononoke" "test"
  > # modify: C "eden/moNOnoke" "test"
  > # modify: D "eden/test" "test"
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=03649f61e7717da595b97394f4788b1d54cc213496adafc56937b544216f1fa6
  C=c8f78ccda6be9fcb22d28ea4380af40784d6869fb6b0b1101c65f6623ff37bfc
  D=240cdc894102f0376b0affc868ee7411ea9f1d687f025febe3868196c1013aca

  $ mononoke_newadmin case-conflict -R repo find-new -i aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  No new case conflicts found
  $ mononoke_newadmin case-conflict -R repo find-new -i 03649f61e7717da595b97394f4788b1d54cc213496adafc56937b544216f1fa6
  No new case conflicts found
  $ mononoke_newadmin case-conflict -R repo find-new -i c8f78ccda6be9fcb22d28ea4380af40784d6869fb6b0b1101c65f6623ff37bfc
  Found new case conflict: (NonRootMPath("eden/moNOnoke"), NonRootMPath("eden/mononoke"))
  $ mononoke_newadmin case-conflict -R repo find-new -i 240cdc894102f0376b0affc868ee7411ea9f1d687f025febe3868196c1013aca
  No new case conflicts found
