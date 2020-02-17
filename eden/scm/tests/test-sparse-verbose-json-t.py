# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# test sparse with --verbose and -T json

sh % "enable sparse"
sh % "hg init myrepo"
sh % "cd myrepo"

sh % "echo a" > "show"
sh % "echo x" > "hide"
sh % "hg ci -Aqm initial"

sh % "echo b" > "show"
sh % "echo y" > "hide"
sh % "echo aa" > "show2"
sh % "echo xx" > "hide2"
sh % "hg ci -Aqm two"

# Verify basic --include and reset

sh % "hg up -q 0"
sh % "hg sparse --include hide -Tjson" == r"""
    [
     {
      "exclude_rules_added": 0,
      "files_added": 0,
      "files_conflicting": 0,
      "files_dropped": 1,
      "include_rules_added": 1,
      "profiles_added": 0
     }
    ]"""
sh % "hg sparse --clear-rules"
sh % "hg sparse --include hide --verbose" == r"""
    calculating actions for refresh
    applying changes to disk (1 actions)
    removing show
    updating dirstate
    Profile # change: 0
    Include rule # change: 1
    Exclude rule # change: 0"""

sh % "hg sparse reset -Tjson" == r"""
    [
     {
      "exclude_rules_added": 0,
      "files_added": 1,
      "files_conflicting": 0,
      "files_dropped": 0,
      "include_rules_added": -1,
      "profiles_added": 0
     }
    ]"""
sh % "hg sparse --include hide"
sh % "hg sparse reset --verbose" == r"""
    calculating actions for refresh
    applying changes to disk (1 actions)
    getting show
    updating dirstate
    Profile # change: 0
    Include rule # change: -1
    Exclude rule # change: 0"""

# Verifying that problematic files still allow us to see the deltas when forcing:

sh % "hg sparse --include 'show*'"
sh % "touch hide"
sh % "hg sparse --delete 'show*' --force -Tjson" == r"""
    pending changes to 'hide'
    [
     {
      "exclude_rules_added": 0,
      "files_added": 0,
      "files_conflicting": 1,
      "files_dropped": 0,
      "include_rules_added": -1,
      "profiles_added": 0
     }
    ]"""
sh % "hg sparse --include 'show*' --force" == "pending changes to 'hide'"
sh % "hg sparse --delete 'show*' --force --verbose" == r"""
    calculating actions for refresh
    verifying no pending changes in newly included files
    pending changes to 'hide'
    applying changes to disk (1 actions)
    updating dirstate
    Profile # change: 0
    Include rule # change: -1
    Exclude rule # change: 0"""
