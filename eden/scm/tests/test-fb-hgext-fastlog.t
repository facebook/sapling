  $ enable tweakdefaults fastlog
  $ setconfig fastlog.enabled=true
  $ readconfig <<EOF
  > [fbscmquery]
  > host=our.intern.facebook.com
  > protocol=http
  > reponame=fbsource
  > path=/intern/conduit/
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
  $ hg log dir -T '{desc}\n'
  a
  $ hg log dir -T '{desc}\n' -M
  a
  $ hg log dir -T '{desc}\n' --all
  a
  $ echo x >> dir/a
  $ hg commit -Aqm a2
  $ hg up -q 0
  $ echo "dog" > dir/b
  $ hg commit -Aqm b
  $ hg log dir -T '{desc}\n'
  b
  a
  $ hg log dir -T '{desc}\n' --all
  b
  a2
  a
  $ hg log dir -r 'draft()' -T '{desc}\n'
  a
  a2
  b

Graphlog still works

  $ hg log dir -G -T '{desc}\n'
  @  b
  │
  o  a
  

  $ hg log dir -G -T '{desc}\n' --all
  @  b
  │
  │ o  a2
  ├─╯
  o  a
  


Create a merge

  $ hg merge --config ui.allowmerge=True
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -T '{desc}\n'
  b
  a
  $ hg commit -Aqm merge
  $ hg log -T '{desc}\n'
  merge
  b
  a2
  a
  $ hg log dir -T '{desc}\n'
  b
  a2
  a
  $ hg log dir -T '{desc}\n' -M
  b
  a2
  a

Test keywords

  $ hg log dir -k 2 -T '{desc}\n'
  a2

Test pruning

  $ hg log dir -P 1 -T '{desc}\n'
  b
  $ hg log dir -P 2 -T '{desc}\n'
  a2

Create a public ancestor
  $ hg up 0 -q
  $ hg debugmakepublic .
  $ hg log dir -T '{desc}\n'
  a
  $ hg up 3 -q
  $ hg log dir -T '{desc}\n'
  b
  a2
  a

Test include / exclude
  $ hg log dir -I 'dir/a' -T '{desc}\n'
  a2
  a
  $ hg log dir -X 'dir/a' -T '{desc}\n'
  b

Log on non-existent directory

  $ hg log dir2
  abort: cannot follow file not in parent revision: "dir2"
  [255]

Start testing with files / multiple directories
  $ mkdir dir2
  $ echo "poo" > dir2/a
  $ hg ci -Aqm dir2-a
  $ hg log -T '{desc}\n' dir dir2
  dir2-a
  b
  a2
  a
  $ echo "food" > dir2/b
  $ hg ci -Aqm dir2-b
  $ hg log -T '{desc}\n' dir dir2
  dir2-b
  dir2-a
  b
  a2
  a

Test globbing

  $ hg log -T '{desc}\n' glob:**a
  dir2-a
  a2
  a
  $ hg log -T '{desc}\n' glob:dir2/**a
  dir2-a

Move directories

  $ mkdir parent
  $ mv dir dir2 parent
  $ hg addremove -q
  $ hg ci -Aqm 'major repo reorg'
  $ hg log -T '{desc} {files}\n' parent
  major repo reorg dir/a dir/b dir2/a dir2/b parent/dir/a parent/dir/b parent/dir2/a parent/dir2/b

File follow behavior

  $ hg log -f -T '{desc}\n' parent/dir/a
  major repo reorg
  a2
  a

Directory follow behavior - not ideal but we don't follow the directory

  $ hg log -f -T '{desc}\n' parent/dir
  major repo reorg

Follow many files

  $ find parent -type f | sort | xargs hg log -f -T '{desc}\n'
  major repo reorg
  dir2-b
  dir2-a
  b
  a2
  a

Globbing with parent

  $ hg log -f -T '{desc}\n' glob:parent/**a
  major repo reorg

Public follow

  $ hg debugmakepublic .
  $ find parent -type f | sort | xargs hg log -f -T '{desc}\n'
  major repo reorg
  dir2-b
  dir2-a
  b
  a2
  a

Multiple public / draft directories

  $ echo "cookies" > parent/dir/c
  $ hg ci -Aqm 'cookies'
  $ echo "treats" > parent/dir2/d
  $ hg ci -Aqm 'treats'
  $ echo "toys" > parent/e
  $ hg ci -Aqm 'toys'
  $ hg log parent/dir -T '{desc}\n'
  cookies
  major repo reorg
  $ hg log parent/dir2 -T '{desc}\n'
  treats
  major repo reorg
  $ hg log parent -T '{desc}\n'
  toys
  treats
  cookies
  major repo reorg
  $ hg log parent/dir parent/dir2 -T '{desc}\n'
  treats
  cookies
  major repo reorg
  $ hg debugmakepublic .
  $ hg log parent/dir -T '{desc}\n'
  cookies
  major repo reorg
  $ hg log parent/dir2 -T '{desc}\n'
  treats
  major repo reorg
  $ hg log parent -T '{desc}\n'
  toys
  treats
  cookies
  major repo reorg
  $ hg log parent/dir parent/dir2 -T '{desc}\n'
  treats
  cookies
  major repo reorg

Globbing with public parent

  $ hg log -T '{desc}\n' glob:parent/*/*
  treats
  cookies
  major repo reorg

Multi-path queries

  $ hg log parent/dir parent/dir2 -T '{node}\n'
  11c9870ffc4024fab11bf166a00b2852ea36bcf6
  5946a2427fdfcb068a8aec1a59227d0d76062b43
  728676e01661ccc3d7e39de054ca3a7288d7e7b6
