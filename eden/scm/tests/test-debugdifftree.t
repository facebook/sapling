#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ newrepo
  $ drawdag << 'EOS'
  > B   # B/dir/B=1
  > |   # B/A=2
  > |   # B/s=x (symlink)
  > |   # B/x= (removed)
  > A   # A/x=1 (executable)
  > EOS

# Print paths:

  $ hg debugdifftree -r "$A" -r "$B"
  M A
  A B
  A dir/B
  A s
  R x

# JSON output:

  $ hg debugdifftree -r $A -r $B -Tjson
  [
   {
    "newflags": "",
    "newnode": "4874dd275af6c7e22b955cdb43f2acc228d5ed29",
    "oldflags": "",
    "oldnode": "005d992c5dcf32993668f7cede29d296c494a5d9",
    "path": "A",
    "status": "M"
   },
   {
    "newflags": "",
    "newnode": "35e7525ce3a48913275d7061dd9a867ffef1e34d",
    "oldflags": "",
    "oldnode": null,
    "path": "B",
    "status": "A"
   },
   {
    "newflags": "",
    "newnode": "f976da1d0df2256cde08db84261621d5e92f77be",
    "oldflags": "",
    "oldnode": null,
    "path": "dir/B",
    "status": "A"
   },
   {
    "newflags": "l",
    "newnode": "d00600e0b09ff8a1909934023a08399f084bc6bc",
    "oldflags": "",
    "oldnode": null,
    "path": "s",
    "status": "A"
   },
   {
    "newflags": "",
    "newnode": null,
    "oldflags": "x",
    "oldnode": "f976da1d0df2256cde08db84261621d5e92f77be",
    "path": "x",
    "status": "R"
   }
  ]

# With path matcher:

  $ hg debugdifftree -r null -r "$B" -Tjson dir
  [
   {
    "newflags": "",
    "newnode": "f976da1d0df2256cde08db84261621d5e92f77be",
    "oldflags": "",
    "oldnode": null,
    "path": "dir/B",
    "status": "A"
   }
  ]
