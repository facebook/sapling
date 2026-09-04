# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ enable debugpathcreation
  $ setconfig subtree.allow-any-source-commit=true
  $ setconfig subtree.min-path-depth=1
  $ newclientrepo

  $ mkdir -p foo1/subdir
  $ echo aaa > foo1/a.txt
  $ echo bbb > foo1/subdir/b.txt
  $ sl commit -Aqm 'add foo1'
  $ A=$(sl log -r . -T '{node}')

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

Follow explicit subtree-copy metadata:

  $ sl subtree copy -r "$A" --from-path foo1 --to-path subtree -m 'subtree copy foo1'
  copying foo1 to subtree
  $ sl debugpathcreation subtree
  tracing backward: 91fb55dfe016 subtree copied 'foo1' to 'subtree'
  75ae72b66962696c45e82d2a43e69188d9930209
  $ sl -q debugpathcreation subtree/subdir
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
