  $ extpath=`dirname $TESTDIR`
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$extpath/hgext3rd/tweakdefaults.py
  > fastlog=$extpath/hgext3rd/fastlog.py
  > fbconduit=$extpath/hgext3rd/fbconduit.py
  > [fbconduit]
  > host=our.intern.facebook.com
  > protocol=http
  > reponame=fbsource
  > path=/intern/conduit/
  > [fastlog]
  > enabled=True
  > EOF

Log on empty repo

  $ hg init repo
  $ cd repo
  $ mkdir dir
  $ hg log dir
  $ hg log dir -M

Create a directory and test some log commands

  $ touch dir/a
  $ hg commit -Aqm a
  $ hg log dir -T '{rev} {desc}\n'
  0 a
  $ hg log dir -T '{rev} {desc}\n' -M
  0 a
  $ hg log dir -T '{rev} {desc}\n' --all
  0 a
  $ echo x >> dir/a
  $ hg commit -Aqm a2
  $ hg up -q 0
  $ touch dir/b
  $ hg commit -Aqm b
  $ hg log dir -T '{rev} {desc}\n'
  2 b
  0 a
  $ hg log dir -T '{rev} {desc}\n' --all
  2 b
  1 a2
  0 a
  $ hg log dir -r 'draft()' -T '{rev} {desc}\n'
  0 a
  1 a2
  2 b

Graphlog still works

  $ hg log dir -G -T '{rev} {desc}\n'
  @  2 b
  |
  o  0 a
  

  $ hg log dir -G -T '{rev} {desc}\n' --all
  @  2 b
  |
  | o  1 a2
  |/
  o  0 a
  

Create a merge

  $ hg merge --config tweakdefaults.allowmerge=True
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -T '{rev} {desc}\n'
  2 b
  0 a
  $ hg commit -Aqm merge
  $ hg log -T '{rev} {desc}\n'
  3 merge
  2 b
  1 a2
  0 a
  $ hg log dir -T '{rev} {desc}\n'
  2 b
  1 a2
  0 a
  $ hg log dir -T '{rev} {desc}\n' -M
  2 b
  1 a2
  0 a

Test keywords

  $ hg log dir -k 2 -T '{rev} {desc}\n'
  1 a2

Test pruning

  $ hg log dir -P 1 -T '{rev} {desc}\n'
  2 b
  $ hg log dir -P 2 -T '{rev} {desc}\n'
  1 a2

Create a public ancestor
  $ hg up 0 -q
  $ hg phase --public .
  $ hg log dir -T '{rev} {desc}\n'
  0 a
  $ hg up 3 -q
  $ hg log dir -T '{rev} {desc}\n'
  2 b
  1 a2
  0 a

Test include / exclude
  $ hg log dir -I 'dir/a' -T '{rev} {desc}\n'
  1 a2
  0 a
  $ hg log dir -X 'dir/a' -T '{rev} {desc}\n'
  2 b

Log on non-existent directory

  $ hg log dir2
  abort: cannot follow file not in parent revision: "dir2"
  [255]

