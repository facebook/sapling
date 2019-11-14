  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=
  > fastlog=
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

  $ echo "pug" > dir/a
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
  $ echo "dog" > dir/b
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

  $ hg merge --config ui.allowmerge=True
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

Start testing with files / multiple directories
  $ mkdir dir2
  $ echo "poo" > dir2/a
  $ hg ci -Aqm dir2-a
  $ hg log -T '{rev} {desc}\n' dir dir2
  4 dir2-a
  2 b
  1 a2
  0 a
  $ echo "food" > dir2/b
  $ hg ci -Aqm dir2-b
  $ hg log -T '{rev} {desc}\n' dir dir2
  5 dir2-b
  4 dir2-a
  2 b
  1 a2
  0 a

Test globbing

  $ hg log -T '{rev} {desc}\n' glob:**a
  4 dir2-a
  1 a2
  0 a
  $ hg log -T '{rev} {desc}\n' glob:dir2/**a
  4 dir2-a

Move directories

  $ mkdir parent
  $ mv dir dir2 parent
  $ hg addremove -q
  $ hg ci -Aqm 'major repo reorg'
  $ hg log -T '{rev} {desc} {files}\n' parent
  6 major repo reorg dir/a dir/b dir2/a dir2/b parent/dir/a parent/dir/b parent/dir2/a parent/dir2/b

File follow behavior

  $ hg log -f -T '{rev} {desc}\n' parent/dir/a
  6 major repo reorg
  1 a2
  0 a

Directory follow behavior - not ideal but we don't follow the directory

  $ hg log -f -T '{rev} {desc}\n' parent/dir
  6 major repo reorg

Follow many files

  $ find parent -type f | sort | xargs hg log -f -T '{rev} {desc}\n'
  6 major repo reorg
  5 dir2-b
  4 dir2-a
  2 b
  1 a2
  0 a

Globbing with parent

  $ hg log -f -T '{rev} {desc}\n' glob:parent/**a
  6 major repo reorg

Public follow

  $ hg phase --public .
  $ find parent -type f | sort | xargs hg log -f -T '{rev} {desc}\n'
  6 major repo reorg
  5 dir2-b
  4 dir2-a
  2 b
  1 a2
  0 a

Multiple public / draft directories

  $ echo "cookies" > parent/dir/c
  $ hg ci -Aqm 'cookies'
  $ echo "treats" > parent/dir2/d
  $ hg ci -Aqm 'treats'
  $ echo "toys" > parent/e
  $ hg ci -Aqm 'toys'
  $ hg log parent/dir -T '{rev} {desc}\n'
  7 cookies
  6 major repo reorg
  $ hg log parent/dir2 -T '{rev} {desc}\n'
  8 treats
  6 major repo reorg
  $ hg log parent -T '{rev} {desc}\n'
  9 toys
  8 treats
  7 cookies
  6 major repo reorg
  $ hg log parent/dir parent/dir2 -T '{rev} {desc}\n'
  8 treats
  7 cookies
  6 major repo reorg
  $ hg phase --public .
  $ hg log parent/dir -T '{rev} {desc}\n'
  7 cookies
  6 major repo reorg
  $ hg log parent/dir2 -T '{rev} {desc}\n'
  8 treats
  6 major repo reorg
  $ hg log parent -T '{rev} {desc}\n'
  9 toys
  8 treats
  7 cookies
  6 major repo reorg
  $ hg log parent/dir parent/dir2 -T '{rev} {desc}\n'
  8 treats
  7 cookies
  6 major repo reorg

Globbing with public parent

  $ hg log -T '{rev} {desc}\n' glob:parent/*/*
  8 treats
  7 cookies
  6 major repo reorg

Multi-path queries

  $ hg log parent/dir parent/dir2 -T '{node}\n'
  11c9870ffc4024fab11bf166a00b2852ea36bcf6
  5946a2427fdfcb068a8aec1a59227d0d76062b43
  728676e01661ccc3d7e39de054ca3a7288d7e7b6
