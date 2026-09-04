# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ enable debugpathcreation
  $ newclientrepo

  $ mkdir -p foo1/subdir
  $ echo aaa > foo1/a.txt
  $ echo bbb > foo1/subdir/b.txt
  $ sl commit -Aqm 'add foo1'

Later changes do not affect the directory's origin:

  $ echo ccc >> foo1/a.txt
  $ sl commit -qm 'modify foo1'
  $ sl debugpathcreation foo1
  75ae72b66962696c45e82d2a43e69188d9930209

Follow ordinary directory copies, including from a nested subdirectory:

  $ sl copy -q foo1 foo2
  $ sl commit -qm 'copy foo1 to foo2'
  $ sl debugpathcreation foo2
  tracing backward: f20c904112ff copied 'foo1' to 'foo2'
  75ae72b66962696c45e82d2a43e69188d9930209
  $ sl -q debugpathcreation foo2/subdir
  75ae72b66962696c45e82d2a43e69188d9930209

Follow chained directory renames:

  $ sl rename -q foo2 foo3
  $ sl commit -qm 'rename foo2 to foo3'
  $ sl -q debugpathcreation foo3
  75ae72b66962696c45e82d2a43e69188d9930209
  $ sl -q debugpathcreation foo3/subdir
  75ae72b66962696c45e82d2a43e69188d9930209

Reject paths that are not tracked directories:

  $ sl debugpathcreation foo3/a.txt
  abort: path 'foo3/a.txt' is not a directory in commit * (glob)
  [255]
  $ sl debugpathcreation missing
  abort: path 'missing' is not a directory in commit * (glob)
  [255]
  $ sl debugpathcreation .
  abort: repository root is not supported
  [255]
