Log on empty repository: checking consistency

  $ hg init empty
  $ cd empty
  $ hg log
  $ hg log -r 1
  abort: unknown revision '1'!
  [255]
  $ hg log -r -1:0
  abort: unknown revision '-1'!
  [255]
  $ hg log -r 'branch(name)'
  abort: unknown revision 'name'!
  [255]
  $ hg log -r null -q
  -1:000000000000

The g is crafted to have 2 filelog topological heads in a linear
changeset graph

  $ hg init a
  $ cd a
  $ echo a > a
  $ echo f > f
  $ hg ci -Ama -d '1 0'
  adding a
  adding f

  $ hg cp a b
  $ hg cp f g
  $ hg ci -mb -d '2 0'

  $ mkdir dir
  $ hg mv b dir
  $ echo g >> g
  $ echo f >> f
  $ hg ci -mc -d '3 0'

  $ hg mv a b
  $ hg cp -f f g
  $ echo a > d
  $ hg add d
  $ hg ci -md -d '4 0'

  $ hg mv dir/b e
  $ hg ci -me -d '5 0'

Make sure largefiles doesn't interfere with logging a regular file
  $ hg --debug log a -T '{rev}: {desc}\n' --config extensions.largefiles=
  updated patterns: ['.hglf/a', 'a']
  0: a
  $ hg log a
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  $ hg log glob:a*
  changeset:   3:2ca5ba701980
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  $ hg --debug log glob:a* -T '{rev}: {desc}\n' --config extensions.largefiles=
  updated patterns: ['glob:.hglf/a*', 'glob:a*']
  3: d
  0: a

log on directory

  $ hg log dir
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  $ hg log somethingthatdoesntexist dir
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  

-f, non-existent directory

  $ hg log -f dir
  abort: cannot follow file not in parent revision: "dir"
  [255]

-f, directory

  $ hg up -q 3
  $ hg log -f dir
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
-f, directory with --patch

  $ hg log -f dir -p
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
  --- /dev/null* (glob)
  +++ b/dir/b* (glob)
  @@ -0,0 +1,1 @@
  +a
  

-f, pattern

  $ hg log -f -I 'dir**' -p
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
  --- /dev/null* (glob)
  +++ b/dir/b* (glob)
  @@ -0,0 +1,1 @@
  +a
  
  $ hg up -q 4

-f, a wrong style

  $ hg log -f -l1 --style something
  abort: style 'something' not found
  (available styles: bisect, changelog, compact, default, phases, status, xml)
  [255]

-f, phases style


  $ hg log -f -l1 --style phases
  changeset:   4:7e4639b4691b
  tag:         tip
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  

  $ hg log -f -l1 --style phases -q
  4:7e4639b4691b

-f, but no args

  $ hg log -f
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  changeset:   3:2ca5ba701980
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

one rename

  $ hg up -q 2
  $ hg log -vf a
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  
  

many renames

  $ hg up -q tip
  $ hg log -vf e
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  description:
  e
  
  
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b f g
  description:
  c
  
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b g
  description:
  b
  
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  
  


log -pf dir/b

  $ hg up -q 3
  $ hg log -pf dir/b
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r 9161b9aeaf16 -r d89b0a12d229 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r 9161b9aeaf16 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

log -pf b inside dir

  $ hg --cwd=dir log -pf b
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r 9161b9aeaf16 -r d89b0a12d229 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r 9161b9aeaf16 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

log -pf, but no args

  $ hg log -pf
  changeset:   3:2ca5ba701980
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  diff -r f8954cd4dc1f -r 2ca5ba701980 a
  --- a/a	Thu Jan 01 00:00:03 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r f8954cd4dc1f -r 2ca5ba701980 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r f8954cd4dc1f -r 2ca5ba701980 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r f8954cd4dc1f -r 2ca5ba701980 g
  --- a/g	Thu Jan 01 00:00:03 1970 +0000
  +++ b/g	Thu Jan 01 00:00:04 1970 +0000
  @@ -1,2 +1,2 @@
   f
  -g
  +f
  
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r d89b0a12d229 -r f8954cd4dc1f b
  --- a/b	Thu Jan 01 00:00:02 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r d89b0a12d229 -r f8954cd4dc1f dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r d89b0a12d229 -r f8954cd4dc1f f
  --- a/f	Thu Jan 01 00:00:02 1970 +0000
  +++ b/f	Thu Jan 01 00:00:03 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +f
  diff -r d89b0a12d229 -r f8954cd4dc1f g
  --- a/g	Thu Jan 01 00:00:02 1970 +0000
  +++ b/g	Thu Jan 01 00:00:03 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +g
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r 9161b9aeaf16 -r d89b0a12d229 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 9161b9aeaf16 -r d89b0a12d229 g
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +f
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r 9161b9aeaf16 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r 000000000000 -r 9161b9aeaf16 f
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +f
  

log -vf dir/b

  $ hg log -vf dir/b
  changeset:   2:f8954cd4dc1f
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b f g
  description:
  c
  
  
  changeset:   1:d89b0a12d229
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b g
  description:
  b
  
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  
  


-f and multiple filelog heads

  $ hg up -q 2
  $ hg log -f g --template '{rev}\n'
  2
  1
  0
  $ hg up -q tip
  $ hg log -f g --template '{rev}\n'
  3
  2
  0


log copies with --copies

  $ hg log -vC --template '{rev} {file_copies}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 

log copies switch without --copies, with old filecopy template

  $ hg log -v --template '{rev} {file_copies_switch%filecopy}\n'
  4 
  3 
  2 
  1 
  0 

log copies switch with --copies

  $ hg log -vC --template '{rev} {file_copies_switch}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 


log copies with hardcoded style and with --style=default

  $ hg log -vC -r4
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  
  
  $ hg log -vC -r4 --style=default
  changeset:   4:7e4639b4691b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  
  
  $ hg log -vC -r4 -Tjson
  [
   {
    "rev": 4,
    "node": "7e4639b4691b9f84b81036a8d4fb218ce3c5e3a3",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [5, 0],
    "desc": "e",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["2ca5ba7019804f1f597249caddf22a64d34df0ba"],
    "files": ["dir/b", "e"],
    "copies": {"e": "dir/b"}
   }
  ]

log copies, non-linear manifest

  $ hg up -C 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv dir/b e
  $ echo foo > foo
  $ hg ci -Ame2 -d '6 0'
  adding foo
  created new head
  $ hg log -v --template '{rev} {file_copies}\n' -r 5
  5 e (dir/b)


log copies, execute bit set

#if execbit
  $ chmod +x e
  $ hg ci -me3 -d '7 0'
  $ hg log -v --template '{rev} {file_copies}\n' -r 6
  6 
#endif


log -p d

  $ hg log -pv d
  changeset:   3:2ca5ba701980
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d g
  description:
  d
  
  
  diff -r f8954cd4dc1f -r 2ca5ba701980 d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  


log --removed file

  $ hg log --removed -v a
  changeset:   3:2ca5ba701980
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d g
  description:
  d
  
  
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  
  

log --removed revrange file

  $ hg log --removed -v -r0:2 a
  changeset:   0:9161b9aeaf16
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  
  
  $ cd ..

log --follow tests

  $ hg init follow
  $ cd follow

  $ echo base > base
  $ hg ci -Ambase -d '1 0'
  adding base

  $ echo r1 >> base
  $ hg ci -Amr1 -d '1 0'
  $ echo r2 >> base
  $ hg ci -Amr2 -d '1 0'

  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b1 > b1

log -r "follow('set:clean()')"

  $ hg log -r "follow('set:clean()')"
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  

  $ hg ci -Amb1 -d '1 0'
  adding b1
  created new head


log -f

  $ hg log -f
  changeset:   3:e62f78d544b4
  tag:         tip
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  

log -r follow('glob:b*')

  $ hg log -r "follow('glob:b*')"
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   3:e62f78d544b4
  tag:         tip
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
log -f -r '1 + 4'

  $ hg up -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b2 > b2
  $ hg ci -Amb2 -d '1 0'
  adding b2
  created new head
  $ hg log -f -r '1 + 4'
  changeset:   4:ddb82e70d1a1
  tag:         tip
  parent:      0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
log -r "follow('set:grep(b2)')"

  $ hg log -r "follow('set:grep(b2)')"
  changeset:   4:ddb82e70d1a1
  tag:         tip
  parent:      0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
log -f -r null

  $ hg log -f -r null
  changeset:   -1:000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  $ hg log -f -r null -G
  o  changeset:   -1:000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000
  


log -f with null parent

  $ hg up -C null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -f


log -r .  with two parents

  $ hg up -C 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -r .
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  


log -r .  with one parent

  $ hg ci -mm12 -d '1 0'
  $ hg log -r .
  changeset:   5:302e9dd6890d
  tag:         tip
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  

  $ echo postm >> b1
  $ hg ci -Amb1.1 -d'1 0'


log --follow-first

  $ hg log --follow-first
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  changeset:   5:302e9dd6890d
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  


log -P 2

  $ hg log -P 2
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  changeset:   5:302e9dd6890d
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  changeset:   4:ddb82e70d1a1
  parent:      0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  


log -r tip -p --git

  $ hg log -r tip -p --git
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  diff --git a/b1 b/b1
  --- a/b1
  +++ b/b1
  @@ -1,1 +1,2 @@
   b1
  +postm
  


log -r ""

  $ hg log -r ''
  hg: parse error: empty query
  [255]

log -r <some unknown node id>

  $ hg log -r 1000000000000000000000000000000000000000
  abort: unknown revision '1000000000000000000000000000000000000000'!
  [255]

log -k r1

  $ hg log -k r1
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
log -p -l2 --color=always

  $ hg --config extensions.color= --config color.mode=ansi \
  >  log -p -l2 --color=always
  \x1b[0;33mchangeset:   6:2404bbcab562\x1b[0m (esc)
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  \x1b[0;1mdiff -r 302e9dd6890d -r 2404bbcab562 b1\x1b[0m (esc)
  \x1b[0;31;1m--- a/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;32;1m+++ b/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;35m@@ -1,1 +1,2 @@\x1b[0m (esc)
   b1
  \x1b[0;32m+postm\x1b[0m (esc)
  
  \x1b[0;33mchangeset:   5:302e9dd6890d\x1b[0m (esc)
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  \x1b[0;1mdiff -r e62f78d544b4 -r 302e9dd6890d b2\x1b[0m (esc)
  \x1b[0;31;1m--- /dev/null	Thu Jan 01 00:00:00 1970 +0000\x1b[0m (esc)
  \x1b[0;32;1m+++ b/b2	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;35m@@ -0,0 +1,1 @@\x1b[0m (esc)
  \x1b[0;32m+b2\x1b[0m (esc)
  


log -r tip --stat

  $ hg log -r tip --stat
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
   b1 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

  $ cd ..

Test that log should respect the order of -rREV even if multiple OR conditions
are specified (issue5100):

  $ hg init revorder
  $ cd revorder

  $ hg branch -q b0
  $ echo 0 >> f0
  $ hg ci -qAm k0 -u u0
  $ hg branch -q b1
  $ echo 1 >> f1
  $ hg ci -qAm k1 -u u1
  $ hg branch -q b2
  $ echo 2 >> f2
  $ hg ci -qAm k2 -u u2

  $ hg update -q b2
  $ echo 3 >> f2
  $ hg ci -qAm k2 -u u2
  $ hg update -q b1
  $ echo 4 >> f1
  $ hg ci -qAm k1 -u u1
  $ hg update -q b0
  $ echo 5 >> f0
  $ hg ci -qAm k0 -u u0

 summary of revisions:

  $ hg log -G -T '{rev} {branch} {author} {desc} {files}\n'
  @  5 b0 u0 k0 f0
  |
  | o  4 b1 u1 k1 f1
  | |
  | | o  3 b2 u2 k2 f2
  | | |
  | | o  2 b2 u2 k2 f2
  | |/
  | o  1 b1 u1 k1 f1
  |/
  o  0 b0 u0 k0 f0
  

 log -b BRANCH in ascending order:

  $ hg log -r0:tip -T '{rev} {branch}\n' -b b0 -b b1
  0 b0
  1 b1
  4 b1
  5 b0
  $ hg log -r0:tip -T '{rev} {branch}\n' -b b1 -b b0
  0 b0
  1 b1
  4 b1
  5 b0

 log --only-branch BRANCH in descending order:

  $ hg log -rtip:0 -T '{rev} {branch}\n' --only-branch b1 --only-branch b2
  4 b1
  3 b2
  2 b2
  1 b1
  $ hg log -rtip:0 -T '{rev} {branch}\n' --only-branch b2 --only-branch b1
  4 b1
  3 b2
  2 b2
  1 b1

 log -u USER in ascending order, against compound set:

  $ hg log -r'::head()' -T '{rev} {author}\n' -u u0 -u u2
  0 u0
  2 u2
  3 u2
  5 u0
  $ hg log -r'::head()' -T '{rev} {author}\n' -u u2 -u u0
  0 u0
  2 u2
  3 u2
  5 u0

 log -k TEXT in descending order, against compound set:

  $ hg log -r'5 + reverse(::3)' -T '{rev} {desc}\n' -k k0 -k k1 -k k2
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0
  $ hg log -r'5 + reverse(::3)' -T '{rev} {desc}\n' -k k2 -k k1 -k k0
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0

 log FILE in ascending order, against dagrange:

  $ hg log -r1:: -T '{rev} {files}\n' f1 f2
  1 f1
  2 f2
  3 f2
  4 f1
  $ hg log -r1:: -T '{rev} {files}\n' f2 f1
  1 f1
  2 f2
  3 f2
  4 f1

  $ cd ..

User

  $ hg init usertest
  $ cd usertest

  $ echo a > a
  $ hg ci -A -m "a" -u "User One <user1@example.org>"
  adding a
  $ echo b > b
  $ hg ci -A -m "b" -u "User Two <user2@example.org>"
  adding b

  $ hg log -u "User One <user1@example.org>"
  changeset:   0:29a4c94f1924
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg log -u "user1" -u "user2"
  changeset:   1:e834b5e69c0e
  tag:         tip
  user:        User Two <user2@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:29a4c94f1924
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg log -u "user3"

  $ cd ..

  $ hg init branches
  $ cd branches

  $ echo a > a
  $ hg ci -A -m "commit on default"
  adding a
  $ hg branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > b
  $ hg ci -A -m "commit on test"
  adding b

  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -A -m "commit on default"
  adding c
  $ hg up test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -A -m "commit on test"
  adding c


log -b default

  $ hg log -b default
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b test

  $ hg log -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b dummy

  $ hg log -b dummy
  abort: unknown revision 'dummy'!
  [255]


log -b .

  $ hg log -b .
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b default -b test

  $ hg log -b default -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b default -b .

  $ hg log -b default -b .
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b . -b test

  $ hg log -b . -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b 2

  $ hg log -b 2
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
#if gettext

Test that all log names are translated (e.g. branches, bookmarks, tags):

  $ hg bookmark babar -r tip

  $ HGENCODING=UTF-8 LANGUAGE=de hg log -r tip
  \xc3\x84nderung:        3:f5d8de11c2e2 (esc)
  Zweig:           test
  Lesezeichen:     babar
  Marke:           tip
  Vorg\xc3\xa4nger:       1:d32277701ccb (esc)
  Nutzer:          test
  Datum:           Thu Jan 01 00:00:00 1970 +0000
  Zusammenfassung: commit on test
  
  $ hg bookmark -d babar

#endif

log -p --cwd dir (in subdir)

  $ mkdir dir
  $ hg log -p --cwd dir
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r d32277701ccb -r f5d8de11c2e2 c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 24427303d56f -r c3a4f03cc9a7 c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r 24427303d56f -r d32277701ccb b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 000000000000 -r 24427303d56f a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  


log -p -R repo

  $ cd dir
  $ hg log -p -R .. ../a
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 000000000000 -r 24427303d56f a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

  $ cd ../..

  $ hg init follow2
  $ cd follow2

# Build the following history:
# tip - o - x - o - x - x
#    \                 /
#     o - o - o - x
#      \     /
#         o
#
# Where "o" is a revision containing "foo" and
# "x" is a revision without "foo"

  $ touch init
  $ hg ci -A -m "init, unrelated"
  adding init
  $ echo 'foo' > init
  $ hg ci -m "change, unrelated"
  $ echo 'foo' > foo
  $ hg ci -A -m "add unrelated old foo"
  adding foo
  $ hg rm foo
  $ hg ci -m "delete foo, unrelated"
  $ echo 'related' > foo
  $ hg ci -A -m "add foo, related"
  adding foo

  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch branch
  $ hg ci -A -m "first branch, unrelated"
  adding branch
  created new head
  $ touch foo
  $ hg ci -A -m "create foo, related"
  adding foo
  $ echo 'change' > foo
  $ hg ci -m "change foo, related"

  $ hg up 6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'change foo in branch' > foo
  $ hg ci -m "change foo in branch, related"
  created new head
  $ hg merge 7
  merging foo
  warning: conflicts while merging foo! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ echo 'merge 1' > foo
  $ hg resolve -m foo
  (no more unresolved files)
  $ hg ci -m "First merge, related"

  $ hg merge 4
  merging foo
  warning: conflicts while merging foo! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ echo 'merge 2' > foo
  $ hg resolve -m foo
  (no more unresolved files)
  $ hg ci -m "Last merge, related"

  $ hg log --graph
  @    changeset:   10:4dae8563d2c5
  |\   tag:         tip
  | |  parent:      9:7b35701b003e
  | |  parent:      4:88176d361b69
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     Last merge, related
  | |
  | o    changeset:   9:7b35701b003e
  | |\   parent:      8:e5416ad8a855
  | | |  parent:      7:87fe3144dcfa
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     First merge, related
  | | |
  | | o  changeset:   8:e5416ad8a855
  | | |  parent:      6:dc6c325fe5ee
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     change foo in branch, related
  | | |
  | o |  changeset:   7:87fe3144dcfa
  | |/   user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     change foo, related
  | |
  | o  changeset:   6:dc6c325fe5ee
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create foo, related
  | |
  | o  changeset:   5:73db34516eb9
  | |  parent:      0:e87515fd044a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     first branch, unrelated
  | |
  o |  changeset:   4:88176d361b69
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add foo, related
  | |
  o |  changeset:   3:dd78ae4afb56
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     delete foo, unrelated
  | |
  o |  changeset:   2:c4c64aedf0f7
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add unrelated old foo
  | |
  o |  changeset:   1:e5faa7440653
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     change, unrelated
  |
  o  changeset:   0:e87515fd044a
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     init, unrelated
  

  $ hg --traceback log -f foo
  changeset:   10:4dae8563d2c5
  tag:         tip
  parent:      9:7b35701b003e
  parent:      4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Last merge, related
  
  changeset:   9:7b35701b003e
  parent:      8:e5416ad8a855
  parent:      7:87fe3144dcfa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First merge, related
  
  changeset:   8:e5416ad8a855
  parent:      6:dc6c325fe5ee
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo in branch, related
  
  changeset:   7:87fe3144dcfa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo, related
  
  changeset:   6:dc6c325fe5ee
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     create foo, related
  
  changeset:   4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  

Also check when maxrev < lastrevfilelog

  $ hg --traceback log -f -r4 foo
  changeset:   4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  
  changeset:   2:c4c64aedf0f7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add unrelated old foo
  
  $ cd ..

Issue2383: hg log showing _less_ differences than hg diff

  $ hg init issue2383
  $ cd issue2383

Create a test repo:

  $ echo a > a
  $ hg ci -Am0
  adding a
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ hg co 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > a
  $ hg ci -m2
  created new head

Merge:

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Make sure there's a file listed in the merge to trigger the bug:

  $ echo c > a
  $ hg ci -m3

Two files shown here in diff:

  $ hg diff --rev 2:3
  diff -r b09be438c43a -r 8e07aafe1edc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r b09be438c43a -r 8e07aafe1edc b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b

Diff here should be the same:

  $ hg log -vpr 3
  changeset:   3:8e07aafe1edc
  tag:         tip
  parent:      2:b09be438c43a
  parent:      1:925d80f479bb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  3
  
  
  diff -r b09be438c43a -r 8e07aafe1edc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r b09be438c43a -r 8e07aafe1edc b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  $ cd ..

'hg log -r rev fn' when last(filelog(fn)) != rev

  $ hg init simplelog
  $ cd simplelog
  $ echo f > a
  $ hg ci -Am'a' -d '0 0'
  adding a
  $ echo f >> a
  $ hg ci -Am'a bis' -d '1 0'

  $ hg log -r0 a
  changeset:   0:9f758d63dcde
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
enable obsolete to test hidden feature

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ hg log --template='{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg debugobsolete a765632148dc55d38c35c4f247c618701886cb2f
  $ hg up null -q
  $ hg log --template='{rev}:{node}\n'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg log --template='{rev}:{node}\n' --hidden
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg log -r a
  abort: hidden revision 'a'!
  (use --hidden to access hidden revisions)
  [255]

test that parent prevent a changeset to be hidden

  $ hg up 1 -q --hidden
  $ hg log --template='{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

test that second parent prevent a changeset to be hidden too

  $ hg debugsetparents 0 1 # nothing suitable to merge here
  $ hg log --template='{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg debugsetparents 1
  $ hg up -q null

bookmarks prevent a changeset being hidden

  $ hg bookmark --hidden -r 1 X
  $ hg log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg bookmark -d X

divergent bookmarks are not hidden

  $ hg bookmark --hidden -r 1 X@foo
  $ hg log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

clear extensions configuration
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=!" >> $HGRCPATH
  $ cd ..

test -u/-k for problematic encoding
# unicode: cp932:
# u30A2    0x83 0x41(= 'A')
# u30C2    0x83 0x61(= 'a')

  $ hg init problematicencoding
  $ cd problematicencoding

  $ python > setup.sh <<EOF
  > print u'''
  > echo a > text
  > hg add text
  > hg --encoding utf-8 commit -u '\u30A2' -m none
  > echo b > text
  > hg --encoding utf-8 commit -u '\u30C2' -m none
  > echo c > text
  > hg --encoding utf-8 commit -u none -m '\u30A2'
  > echo d > text
  > hg --encoding utf-8 commit -u none -m '\u30C2'
  > '''.encode('utf-8')
  > EOF
  $ sh < setup.sh

test in problematic encoding
  $ python > test.sh <<EOF
  > print u'''
  > hg --encoding cp932 log --template '{rev}\\n' -u '\u30A2'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -u '\u30C2'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -k '\u30A2'
  > echo ====
  > hg --encoding cp932 log --template '{rev}\\n' -k '\u30C2'
  > '''.encode('cp932')
  > EOF
  $ sh < test.sh
  0
  ====
  1
  ====
  2
  0
  ====
  3
  1

  $ cd ..

test hg log on non-existent files and on directories
  $ hg init issue1340
  $ cd issue1340
  $ mkdir d1; mkdir D2; mkdir D3.i; mkdir d4.hg; mkdir d5.d; mkdir .d6
  $ echo 1 > d1/f1
  $ echo 1 > D2/f1
  $ echo 1 > D3.i/f1
  $ echo 1 > d4.hg/f1
  $ echo 1 > d5.d/f1
  $ echo 1 > .d6/f1
  $ hg -q add .
  $ hg commit -m "a bunch of weird directories"
  $ hg log -l1 d1/f1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 f1
  $ hg log -l1 . | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 ./ | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 d1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 D2 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 D2/f1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 D3.i | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 D3.i/f1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 d4.hg | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 d4.hg/f1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 d5.d | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 d5.d/f1 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 .d6 | grep changeset
  changeset:   0:65624cd9070a
  $ hg log -l1 .d6/f1 | grep changeset
  changeset:   0:65624cd9070a

issue3772: hg log -r :null showing revision 0 as well

  $ hg log -r :null
  changeset:   0:65624cd9070a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  
  changeset:   -1:000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  $ hg log -r null:null
  changeset:   -1:000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
working-directory revision requires special treatment

clean:

  $ hg log -r 'wdir()' --debug
  changeset:   2147483647:ffffffffffffffffffffffffffffffffffffffff
  phase:       draft
  parent:      0:65624cd9070a035fa7191a54f2b8af39f16b0c08
  parent:      -1:0000000000000000000000000000000000000000
  user:        test
  date:        [A-Za-z0-9:+ ]+ (re)
  extra:       branch=default
  
  $ hg log -r 'wdir()' -p --stat
  changeset:   2147483647:ffffffffffff
  parent:      0:65624cd9070a
  user:        test
  date:        [A-Za-z0-9:+ ]+ (re)
  
  
  

dirty:

  $ echo 2 >> d1/f1
  $ echo 2 > d1/f2
  $ hg add d1/f2
  $ hg remove .d6/f1
  $ hg status
  M d1/f1
  A d1/f2
  R .d6/f1

  $ hg log -r 'wdir()'
  changeset:   2147483647:ffffffffffff
  parent:      0:65624cd9070a
  user:        test
  date:        [A-Za-z0-9:+ ]+ (re)
  
  $ hg log -r 'wdir()' -q
  2147483647:ffffffffffff

  $ hg log -r 'wdir()' --debug
  changeset:   2147483647:ffffffffffffffffffffffffffffffffffffffff
  phase:       draft
  parent:      0:65624cd9070a035fa7191a54f2b8af39f16b0c08
  parent:      -1:0000000000000000000000000000000000000000
  user:        test
  date:        [A-Za-z0-9:+ ]+ (re)
  files:       d1/f1
  files+:      d1/f2
  files-:      .d6/f1
  extra:       branch=default
  
  $ hg log -r 'wdir()' -p --stat --git
  changeset:   2147483647:ffffffffffff
  parent:      0:65624cd9070a
  user:        test
  date:        [A-Za-z0-9:+ ]+ (re)
  
   .d6/f1 |  1 -
   d1/f1  |  1 +
   d1/f2  |  1 +
   3 files changed, 2 insertions(+), 1 deletions(-)
  
  diff --git a/.d6/f1 b/.d6/f1
  deleted file mode 100644
  --- a/.d6/f1
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -1
  diff --git a/d1/f1 b/d1/f1
  --- a/d1/f1
  +++ b/d1/f1
  @@ -1,1 +1,2 @@
   1
  +2
  diff --git a/d1/f2 b/d1/f2
  new file mode 100644
  --- /dev/null
  +++ b/d1/f2
  @@ -0,0 +1,1 @@
  +2
  
  $ hg log -r 'wdir()' -Tjson
  [
   {
    "rev": null,
    "node": null,
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [*, 0], (glob)
    "desc": "",
    "bookmarks": [],
    "tags": [],
    "parents": ["65624cd9070a035fa7191a54f2b8af39f16b0c08"]
   }
  ]

  $ hg log -r 'wdir()' -Tjson -q
  [
   {
    "rev": null,
    "node": null
   }
  ]

  $ hg log -r 'wdir()' -Tjson --debug
  [
   {
    "rev": null,
    "node": null,
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [*, 0], (glob)
    "desc": "",
    "bookmarks": [],
    "tags": [],
    "parents": ["65624cd9070a035fa7191a54f2b8af39f16b0c08"],
    "manifest": null,
    "extra": {"branch": "default"},
    "modified": ["d1/f1"],
    "added": ["d1/f2"],
    "removed": [".d6/f1"]
   }
  ]

  $ hg revert -aqC

Check that adding an arbitrary name shows up in log automatically

  $ cat > ../names.py <<EOF
  > """A small extension to test adding arbitrary names to a repo"""
  > from mercurial.namespaces import namespace
  > 
  > def reposetup(ui, repo):
  >     foo = {'foo': repo[0].node()}
  >     names = lambda r: foo.keys()
  >     namemap = lambda r, name: foo.get(name)
  >     nodemap = lambda r, node: [name for name, n in foo.iteritems()
  >                                if n == node]
  >     ns = namespace("bars", templatename="bar", logname="barlog",
  >                    colorname="barcolor", listnames=names, namemap=namemap,
  >                    nodemap=nodemap)
  > 
  >     repo.names.addnamespace(ns)
  > EOF

  $ hg --config extensions.names=../names.py log -r 0
  changeset:   0:65624cd9070a
  tag:         tip
  barlog:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  
  $ hg --config extensions.names=../names.py \
  >  --config extensions.color= --config color.log.barcolor=red \
  >  --color=always log -r 0
  \x1b[0;33mchangeset:   0:65624cd9070a\x1b[0m (esc)
  tag:         tip
  \x1b[0;31mbarlog:      foo\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  
  $ hg --config extensions.names=../names.py log -r 0 --template '{bars}\n'
  foo

  $ cd ..

hg log -f dir across branches

  $ hg init acrossbranches
  $ cd acrossbranches
  $ mkdir d
  $ echo a > d/a && hg ci -Aqm a
  $ echo b > d/a && hg ci -Aqm b
  $ hg up -q 0
  $ echo b > d/a && hg ci -Aqm c
  $ hg log -f d -T '{desc}' -G
  @  c
  |
  o  a
  
Ensure that largefiles doesn't interfere with following a normal file
  $ hg  --config extensions.largefiles= log -f d -T '{desc}' -G
  @  c
  |
  o  a
  
  $ hg log -f d/a -T '{desc}' -G
  @  c
  |
  o  a
  
  $ cd ..

hg log -f with linkrev pointing to another branch
-------------------------------------------------

create history with a filerev whose linkrev points to another branch

  $ hg init branchedlinkrev
  $ cd branchedlinkrev
  $ echo 1 > a
  $ hg commit -Am 'content1'
  adding a
  $ echo 2 > a
  $ hg commit -m 'content2'
  $ hg up --rev 'desc(content1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo unrelated > unrelated
  $ hg commit -Am 'unrelated'
  adding unrelated
  created new head
  $ hg graft -r 'desc(content2)'
  grafting 1:2294ae80ad84 "content2"
  $ echo 3 > a
  $ hg commit -m 'content3'
  $ hg log -G
  @  changeset:   4:50b9b36e9c5d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content3
  |
  o  changeset:   3:15b2327059e5
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content2
  |
  o  changeset:   2:2029acd1168c
  |  parent:      0:ae0a3c9f9e95
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     unrelated
  |
  | o  changeset:   1:2294ae80ad84
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     content2
  |
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

log -f on the file should list the graft result.

  $ hg log -Gf a
  @  changeset:   4:50b9b36e9c5d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content3
  |
  o  changeset:   3:15b2327059e5
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     content2
  :
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

plain log lists the original version
(XXX we should probably list both)

  $ hg log -G a
  @  changeset:   4:50b9b36e9c5d
  :  tag:         tip
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     content3
  :
  : o  changeset:   1:2294ae80ad84
  :/   user:        test
  :    date:        Thu Jan 01 00:00:00 1970 +0000
  :    summary:     content2
  :
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

hg log -f from the grafted changeset
(The bootstrap should properly take the topology in account)

  $ hg up 'desc(content3)^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -Gf a
  @  changeset:   3:15b2327059e5
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     content2
  :
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

Test that we use the first non-hidden changeset in that case.

(hide the changeset)

  $ hg log -T '{node}\n' -r 1
  2294ae80ad8447bc78383182eeac50cb049df623
  $ hg debugobsolete 2294ae80ad8447bc78383182eeac50cb049df623
  $ hg log -G
  o  changeset:   4:50b9b36e9c5d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content3
  |
  @  changeset:   3:15b2327059e5
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content2
  |
  o  changeset:   2:2029acd1168c
  |  parent:      0:ae0a3c9f9e95
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     unrelated
  |
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

Check that log on the file does not drop the file revision.

  $ hg log -G a
  o  changeset:   4:50b9b36e9c5d
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     content3
  |
  @  changeset:   3:15b2327059e5
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     content2
  :
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

Even when a head revision is linkrev-shadowed.

  $ hg log -T '{node}\n' -r 4
  50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ hg debugobsolete 50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ hg log -G a
  @  changeset:   3:15b2327059e5
  :  tag:         tip
  :  user:        test
  :  date:        Thu Jan 01 00:00:00 1970 +0000
  :  summary:     content2
  :
  o  changeset:   0:ae0a3c9f9e95
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1
  

  $ cd ..

Even when the file revision is missing from some head:

  $ hg init issue4490
  $ cd issue4490
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'evolution=createmarkers' >> .hg/hgrc
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ echo B > b
  $ hg ci --amend -m 1
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -Am2
  adding c
  created new head
  $ hg up 'head() and not .'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G
  o  changeset:   4:db815d6d32e6
  |  tag:         tip
  |  parent:      0:f7b1eb17ad24
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     2
  |
  | @  changeset:   3:9bc8ce7f9356
  |/   parent:      0:f7b1eb17ad24
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1
  |
  o  changeset:   0:f7b1eb17ad24
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0
  
  $ hg log -f -G b
  @  changeset:   3:9bc8ce7f9356
  |  parent:      0:f7b1eb17ad24
  ~  user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  
  $ hg log -G b
  @  changeset:   3:9bc8ce7f9356
  |  parent:      0:f7b1eb17ad24
  ~  user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  
  $ cd ..

Check proper report when the manifest changes but not the file issue4499
------------------------------------------------------------------------

  $ hg init issue4499
  $ cd issue4499
  $ for f in A B C D F E G H I J K L M N O P Q R S T U; do
  >     echo 1 > $f;
  >     hg add $f;
  > done
  $ hg commit -m 'A1B1C1'
  $ echo 2 > A
  $ echo 2 > B
  $ echo 2 > C
  $ hg commit -m 'A2B2C2'
  $ hg up 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > A
  $ echo 2 > B
  $ echo 2 > C
  $ hg commit -m 'A3B2C2'
  created new head

  $ hg log -G
  @  changeset:   2:fe5fc3d0eb17
  |  tag:         tip
  |  parent:      0:abf4f0e38563
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     A3B2C2
  |
  | o  changeset:   1:07dcc6b312c0
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     A2B2C2
  |
  o  changeset:   0:abf4f0e38563
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1
  

Log -f on B should reports current changesets

  $ hg log -fG B
  @  changeset:   2:fe5fc3d0eb17
  |  tag:         tip
  |  parent:      0:abf4f0e38563
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     A3B2C2
  |
  o  changeset:   0:abf4f0e38563
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1
  
  $ cd ..
