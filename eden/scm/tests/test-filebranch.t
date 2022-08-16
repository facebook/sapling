#chg-compatible
#debugruntest-compatible

  $ configure modernclient

This test makes sure that we don't mark a file as merged with its ancestor
when we do a merge.

  $ cat <<EOF > merge
  > from __future__ import print_function
  > import sys, os
  > print("merging for", os.path.basename(sys.argv[1]))
  > EOF
  $ HGMERGE="$PYTHON ../merge"; export HGMERGE

Creating base:

  $ newclientrepo a
  $ echo 1 > foo
  $ echo 1 > bar
  $ echo 1 > baz
  $ echo 1 > quux
  $ hg add foo bar baz quux
  $ hg commit -m "base"
  $ hg push -q -r . --to book --create

  $ newclientrepo b test:a_server book

Creating branch a:

  $ cd ../a
  $ echo 2a > foo
  $ echo 2a > bar
  $ hg commit -m "branch a"
  $ hg push -q -r . --to book --create

Creating branch b:

  $ cd ../b
  $ echo 2b > foo
  $ echo 2b > baz
  $ hg commit -m "branch b"

We shouldn't have anything but n state here:

  $ hg debugstate --nodates | grep -v "^n"
  [1]

Merging:

  $ hg pull test:a_server
  pulling from test:a_server
  searching for changes

  $ hg merge -v
  resolving manifests
  merging foo
  merging for foo
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ echo 2m > foo
  $ echo 2b > baz
  $ echo new > quux

  $ hg ci -m "merge"

  $ hg log foo --graph -T '{desc}'
  warning: file log can be slow on large repos - use -f to speed it up
  @    merge
  ├─╮
  │ o  branch a
  │ │
  o │  branch b
  ├─╯
  o  base
  
  $ hg log bar --graph -T '{desc}'
  warning: file log can be slow on large repos - use -f to speed it up
  o  branch a
  │
  o  base
  
  $ hg log baz --graph -T '{desc}'
  warning: file log can be slow on large repos - use -f to speed it up
  o  branch b
  │
  o  base
  
  $ hg log quux --graph -T '{desc}'
  warning: file log can be slow on large repos - use -f to speed it up
  @  merge
  ╷
  o  base
  

log should show foo and quux changed:

  $ hg log -v -r tip
  commit:      d8a521142a3c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo quux
  description:
  merge
  
  

Manifest entries should match tips of all files:

  $ hg manifest --debug
  33d1fb69067a0139622a3fa3b7ba1cdb1367972e 644   bar
  2ffeddde1b65b4827f6746174a145474129fa2ce 644   baz
  aa27919ee4303cfd575e1fb932dd64d75aa08be4 644   foo
  6128c0f33108e8cfbb4e0824d13ae48b466d7280 644   quux

Everything should be clean now:

  $ hg status
