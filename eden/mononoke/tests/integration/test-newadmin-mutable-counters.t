# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

Validate if we can create new mutable counters
  $ mononoke_newadmin mutable-counters -R repo set foo 7
  Value of foo in repo repo(Id: 0) set to 7
  $ mononoke_newadmin mutable-counters -R repo set bar 9
  Value of bar in repo repo(Id: 0) set to 9

Validate if we can update an existing counter
  $ mononoke_newadmin mutable-counters -R repo set foo 10 --prev-value 7
  Value of foo in repo repo(Id: 0) set to 10

Validate if we get an error trying to update an existing counter with incorrect previous value
  $ mononoke_newadmin mutable-counters -R repo set foo 12 --prev-value 8
  Value of foo in repo repo(Id: 0) was NOT set to 12. The previous value of the counter did not match Some(8)

Validate if all the new added counters are present
  $ mononoke_newadmin mutable-counters -R repo list
  bar                           =9
  foo                           =10
  $ mononoke_newadmin mutable-counters -R repo get bar
  Some(9)
  $ mononoke_newadmin mutable-counters -R repo get baz
  None
