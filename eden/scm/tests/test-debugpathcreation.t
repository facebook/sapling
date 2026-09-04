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

  $ echo 1 > foo3/1.txt
  $ echo 2 > foo3/2.txt
  $ echo 3 > foo3/3.txt
  $ echo 4 > foo3/4.txt
  $ echo 5 > foo3/5.txt
  $ echo 6 > foo3/6.txt
  $ echo 7 > foo3/7.txt
  $ echo 8 > foo3/8.txt
  $ sl commit -Aqm 'add more files to foo3'

Allow a few new files in a copied directory:

  $ sl copy -q foo3 mixed
  $ echo new > mixed/new.txt
  $ sl add -q mixed/new.txt
  $ sl commit -qm 'copy foo3 and add a file'
  $ sl -q debugpathcreation mixed
  75ae72b66962696c45e82d2a43e69188d9930209

Allow copied files to be deleted before committing:

  $ sl copy -q foo3 pruned
  $ sl forget -q pruned/a.txt
  $ rm pruned/a.txt
  $ sl commit -qm 'copy foo3 without one file'
  $ sl -q debugpathcreation pruned
  75ae72b66962696c45e82d2a43e69188d9930209
  $ sl -q debugpathcreation pruned/subdir
  75ae72b66962696c45e82d2a43e69188d9930209

Do not infer a directory copy when fewer than 90% of destination files map:

  $ sl copy -q foo3 weak-mapping
  $ sl forget -q weak-mapping/1.txt weak-mapping/2.txt
  $ echo new1 > weak-mapping/1.txt
  $ echo new2 > weak-mapping/2.txt
  $ sl add -q weak-mapping/1.txt weak-mapping/2.txt
  $ sl commit -qm 'copy foo3 with too many new files'
  $ sl debugpathcreation weak-mapping
  f03bffcc5680263863bbbeb1c026f3ebf99287ce

Warn but continue when source and destination sizes differ by more than 10%:

  $ sl copy -q foo3 weak-size
  $ sl forget -q weak-size/1.txt weak-size/2.txt
  $ rm weak-size/1.txt weak-size/2.txt
  $ sl commit -qm 'copy too little of foo3'
  $ sl -q debugpathcreation weak-size
  warning: inferred directory copy from 'foo3' to 'weak-size' despite dissimilar file counts (10 source, 8 destination)
  75ae72b66962696c45e82d2a43e69188d9930209

Follow explicit subtree-copy metadata:

  $ sl subtree copy -r "$A" --from-path foo1 --to-path subtree -m 'subtree copy foo1'
  copying foo1 to subtree
  $ sl debugpathcreation subtree
  tracing backward: 5fa5d1947ca7 subtree copied 'foo1' to 'subtree'
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
