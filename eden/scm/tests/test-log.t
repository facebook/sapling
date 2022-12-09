#debugruntest-compatible
# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

# Log on empty repository: checking consistency

  $ setconfig devel.segmented-changelog-rev-compat=true

#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

  $ hg init empty
  $ cd empty
  $ hg log
  $ hg log -r 1
  abort: unknown revision '1'!
  [255]
  $ hg log -r '-1:0'
  abort: unknown revision '-1'!
  [255]
  $ hg log -r 'branch(name)'
  $ hg log -r null -q
  000000000000

  $ cd ..

# The g is crafted to have 2 filelog topological heads in a linear
# changeset graph

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

  $ hg --debug log a -T '{rev}: {desc}\n'
  0: a
  $ hg log a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  $ hg log 'glob:a*'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  $ hg --debug log 'glob:a*' -T '{rev}: {desc}\n'
  3: d
  0: a

# log on directory

  $ hg log dir
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  $ hg log somethingthatdoesntexist dir
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c

# -f, non-existent directory

  $ hg log -f dir
  abort: cannot follow file not in parent revision: "dir"
  [255]

# -f, directory
# (The code path using "follow()" revset will follow file renames, so 'b' and 'a' show up)

  $ hg up -q 3
  $ hg log -f dir
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a

# -f, directory with --patch

  $ hg log -f dir -p
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a

# -f, pattern

  $ hg log -f -I 'dir**' -p
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  $ hg up -q 4

# -f, a wrong style

  $ hg log -f -l1 --style something
  abort: style 'something' not found
  (available styles: bisect, changelog, compact, default, phases, show, sl_default, status, xml)
  [255]

# -f, phases style

  $ hg log -f -l1 --style phases
  commit:      * (glob)
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e

  $ hg log -f -l1 --style phases -q
  * (glob)

# -f, but no args

  $ hg log -f
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a

# one rename

  $ hg up -q 2
  $ hg log -vf a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# many renames

  $ hg up -q tip
  $ hg log -vf e
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  description:
  e
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b f g
  description:
  c
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b g
  description:
  b
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# log -pf dir/b

  $ hg up -q 3
  $ hg log -pf dir/b
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r * -r * a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a

# log -pf b inside dir

  $ hg '--cwd=dir' log -pf b
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r * -r * a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a

# log -pf, but no args

  $ hg log -pf
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  diff -r * -r * a (glob)
  --- a/a	Thu Jan 01 00:00:03 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r * -r * d (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r * -r * g (glob)
  --- a/g	Thu Jan 01 00:00:03 1970 +0000
  +++ b/g	Thu Jan 01 00:00:04 1970 +0000
  @@ -1,2 +1,2 @@
   f
  -g
  +f
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * b (glob)
  --- a/b	Thu Jan 01 00:00:02 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r * -r * f (glob)
  --- a/f	Thu Jan 01 00:00:02 1970 +0000
  +++ b/f	Thu Jan 01 00:00:03 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +f
  diff -r * -r * g (glob)
  --- a/g	Thu Jan 01 00:00:02 1970 +0000
  +++ b/g	Thu Jan 01 00:00:03 1970 +0000
  @@ -1,1 +1,2 @@
   f
  +g
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r * -r * g (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/g	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +f
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r * -r * a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  diff -r * -r * f (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +f

# log -vf dir/b

  $ hg log -vf dir/b
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b f g
  description:
  c
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b g
  description:
  b
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# -f and multiple filelog heads

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

# log copies with --copies

  $ hg log -vC --template '{rev} {file_copies}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 

# log copies switch without --copies, with old filecopy template

  $ hg log -v --template '{rev} {file_copies_switch%filecopy}\n'
  4 
  3 
  2 
  1 
  0 

# log copies switch with --copies

  $ hg log -vC --template '{rev} {file_copies_switch}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 

# log copies with hardcoded style and with --style=default

  $ hg log -vC -r4
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  $ hg log -vC -r4 '--style=default'
  commit:      * (glob)
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
    "node": "*", (glob)
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [5, 0],
    "desc": "e",
    "bookmarks": [],
    "parents": ["*"], (glob)
    "files": ["dir/b", "e"],
    "copies": {"e": "dir/b"}
   }
  ]

# log copies, non-linear manifest

  $ hg up -C 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv dir/b e
  $ echo foo > foo
  $ hg ci -Ame2 -d '6 0'
  adding foo
  $ hg log -v --template '{rev} {file_copies}\n' -r 5
  5 e (dir/b)

#if execbit
  $ chmod +x e
  $ hg ci -me3 -d '7 0'
  $ hg log -v --template '{rev} {file_copies}\n' -r 6
  6 
#endif

# log -p d

  $ hg log -pv d
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d g
  description:
  d
  
  
  diff -r * -r * d (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a

# log --removed file

  $ hg log --removed -v a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d g
  description:
  d
  
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# log --removed revrange file

  $ hg log --removed -v '-r0:2' a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a
  $ cd ..

# log --follow tests

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

# log -r "follow('set:clean()')"

  $ hg log -r 'follow('\''set:clean()'\'')'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1

  $ hg ci -Amb1 -d '1 0'
  adding b1

# log -f

  $ hg log -f
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base

# log -r follow('glob:b*')

  $ hg log -r 'follow('\''glob:b*'\'')'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1

# log -f -r '1 + 4'

  $ hg up -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b2 > b2
  $ hg ci -Amb2 -d '1 0'
  adding b2
  $ hg log -f -r '1 + 4'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base

# log -r "follow('set:grep(b2)')"

  $ hg log -r 'follow('\''set:grep(b2)'\'')'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2

# log -r "follow('set:grep(b2)', 4)"

  $ hg up -qC 0
  $ hg log -r 'follow('\''set:grep(b2)'\'', 4)'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2

# follow files starting from multiple revisions:

  $ hg log -T '{rev}: {files}\n' -r 'follow('\''glob:b?'\'', startrev=2+3+4)'
  3: b1
  4: b2

# follow files starting from empty revision:

  $ hg log -T '{rev}: {files}\n' -r 'follow('\''glob:*'\'', startrev=.-.)'

# follow starting from revisions:

  $ hg log -Gq -r 'follow(startrev=2+4)'
  o  ddb82e70d1a1
  │
  │ o  60c670bf5b30
  │ │
  │ o  3d5bf5654eda
  ├─╯
  @  67e992f2c4f3

# follow the current revision:

  $ hg log -Gq -r 'follow()'
  @  67e992f2c4f3

  $ hg up -qC 4

# log -f -r null

  $ hg log -f -r null
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  $ hg log -f -r null -G
  o  commit:      000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000

# log -f with null parent

  $ hg up -C null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -f

# log -r .  with two parents

  $ hg up -C 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -r .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1

# log -r .  with one parent

  $ hg ci -mm12 -d '1 0'
  $ hg log -r .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12

  $ echo postm >> b1
  $ hg ci -Amb1.1 '-d1 0'

# log --follow-first

  $ hg log --follow-first
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base

# log -P 2

  $ hg log -P 2
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1

# log -r tip -p --git

  $ hg log -r tip -p --git
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  diff --git a/b1 b/b1
  --- a/b1
  +++ b/b1
  @@ -1,1 +1,2 @@
   b1
  +postm

# log -r ""

  $ hg log -r ''
  hg: parse error: empty query
  [255]

# log -r <some unknown node id>

  $ hg log -r 1000000000000000000000000000000000000000
  abort: unknown revision '1000000000000000000000000000000000000000'!
  [255]

# log -k r1

  $ hg log -k r1
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1

# log -p -l2 --color=always

  $ hg --config 'extensions.color=' --config 'color.mode=ansi' log -p -l2 '--color=always'
  \x1b[0m\x1b[1m\x1b[93mcommit:      2404bbcab562\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  \x1b[0m\x1b[1mdiff -r 302e9dd6890d -r 2404bbcab562 b1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[31m--- a/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32m+++ b/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[35m@@ -1,1 +1,2 @@\x1b[39m (esc)
   b1
  \x1b[92m+postm\x1b[39m (esc)
  
  \x1b[0m\x1b[1m\x1b[93mcommit:      302e9dd6890d\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  \x1b[0m\x1b[1mdiff -r e62f78d544b4 -r 302e9dd6890d b2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[31m--- /dev/null	Thu Jan 01 00:00:00 1970 +0000\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32m+++ b/b2	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[35m@@ -0,0 +1,1 @@\x1b[39m (esc)
  \x1b[92m+b2\x1b[39m (esc)

# log -r tip --stat

  $ hg log -r tip --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
   b1 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ cd ..

# log --follow --patch FILE in repository where linkrev isn't trustworthy
# (issue5376)

  $ hg init follow-dup
  $ cd follow-dup
  $ cat >> .hg/hgrc << 'EOF'
  > [ui]
  > logtemplate = '=== {rev}: {desc}\n'
  > [diff]
  > nodates = True
  > EOF
  $ echo 0 >> a
  $ hg ci -qAm a0
  $ echo 1 >> a
  $ hg ci -m a1
  $ hg up -q 0
  $ echo 1 >> a
  $ touch b
  $ hg ci -qAm 'a1 with b'
  $ echo 3 >> a
  $ hg ci -m a3

#  fctx.rev() == 2, but fctx.linkrev() == 1

  $ hg log -pf a
  === 3: a3
  diff -r * -r * a (glob)
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   0
   1
  +3
  
  === 2: a1 with b
  diff -r * -r * a (glob)
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   0
  +1
  
  === 0: a0
  diff -r * -r * a (glob)
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +0

#  fctx.introrev() == 2, but fctx.linkrev() == 1

  $ hg up -q 2
  $ hg log -pf a
  === 2: a1 with b
  diff -r * -r * a (glob)
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   0
  +1
  
  === 0: a0
  diff -r * -r * a (glob)
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +0

  $ cd ..

# Multiple copy sources of a file:

  $ hg init follow-multi
  $ cd follow-multi
  $ echo 0 >> a
  $ hg ci -qAm a
  $ hg cp a b
  $ hg ci -m 'a->b'
  $ echo 2 >> a
  $ hg ci -m a
  $ echo 3 >> b
  $ hg ci -m b
  $ echo 4 >> a
  $ echo 4 >> b
  $ hg ci -m 'a,b'
  $ echo 5 >> a
  $ hg ci -m a0
  $ echo 6 >> b
  $ hg ci -m b0
  $ hg up -q 4
  $ echo 7 >> b
  $ hg ci -m b1
  $ echo 8 >> a
  $ hg ci -m a1
  $ hg rm a
  $ hg mv b a
  $ hg ci -m 'b1->a1'
  $ hg merge -qt ':local'
  $ hg ci -m '(a0,b1->a1)->a'

  $ hg log -GT '{rev}: {desc}\n'
  @    10: (a0,b1->a1)->a
  ├─╮
  │ o  9: b1->a1
  │ │
  │ o  8: a1
  │ │
  │ o  7: b1
  │ │
  o │  6: b0
  │ │
  o │  5: a0
  ├─╯
  o  4: a,b
  │
  o  3: b
  │
  o  2: a
  │
  o  1: a->b
  │
  o  0: a

#  since file 'a' has multiple copy sources at the revision 4, ancestors can't
#  be indexed solely by fctx.linkrev().

  $ hg log -T '{rev}: {desc}\n' -f a
  10: (a0,b1->a1)->a
  9: b1->a1
  7: b1
  5: a0
  4: a,b
  3: b
  2: a
  1: a->b
  0: a

  $ cd ..

# Test that log should respect the order of -rREV even if multiple OR conditions
# are specified (issue5100):

  $ hg init revorder
  $ cd revorder

  $ hg book -q b0
  $ echo 0 >> f0
  $ hg ci -qAm k0 -u u0
  $ hg book -q b1
  $ echo 1 >> f1
  $ hg ci -qAm k1 -u u1
  $ hg book -q b2
  $ echo 2 >> f2
  $ hg ci -qAm k2 -u u2

  $ hg goto -q b2
  $ echo 3 >> f2
  $ hg ci -qAm k2 -u u2
  $ hg goto -q b1
  $ echo 4 >> f1
  $ hg ci -qAm k1 -u u1
  $ hg goto -q b0
  $ echo 5 >> f0
  $ hg ci -qAm k0 -u u0

#  summary of revisions:

  $ hg log -G -T '{rev} {bookmarks} {author} {desc} {files}\n'
  @  5 b0 u0 k0 f0
  │
  │ o  4 b1 u1 k1 f1
  │ │
  │ │ o  3 b2 u2 k2 f2
  │ │ │
  │ │ o  2  u2 k2 f2
  │ ├─╯
  │ o  1  u1 k1 f1
  ├─╯
  o  0  u0 k0 f0

#  log -u USER in ascending order, against compound set:

  $ hg log '-r::head()' -T '{rev} {author}\n' -u u0 -u u2
  0 u0
  2 u2
  3 u2
  5 u0
  $ hg log '-r::head()' -T '{rev} {author}\n' -u u2 -u u0
  0 u0
  2 u2
  3 u2
  5 u0

#  log -k TEXT in descending order, against compound set:

  $ hg log '-r5 + reverse(::3)' -T '{rev} {desc}\n' -k k0 -k k1 -k k2
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0
  $ hg log '-r5 + reverse(::3)' -T '{rev} {desc}\n' -k k2 -k k1 -k k0
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0

#  log FILE in ascending order, against dagrange:

  $ hg log '-r1::' -T '{rev} {files}\n' f1 f2
  1 f1
  2 f2
  3 f2
  4 f1
  $ hg log '-r1::' -T '{rev} {files}\n' f2 f1
  1 f1
  2 f2
  3 f2
  4 f1

  $ cd ..

# User

  $ hg init usertest
  $ cd usertest

  $ echo a > a
  $ hg ci -A -m a -u 'User One <user1@example.org>'
  adding a
  $ echo b > b
  $ hg ci -A -m b -u 'User Two <user2@example.org>'
  adding b

  $ hg log -u 'User One <user1@example.org>'
  commit:      * (glob)
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  $ hg log -u user1 -u user2
  commit:      * (glob)
  user:        User Two <user2@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  commit:      * (glob)
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  $ hg log -u user3

  $ cd ..

  $ hg init branches
  $ cd branches

  $ echo a > a
  $ hg ci -A -m 'commit on default'
  adding a
  $ hg book test
  $ echo b > b
  $ hg ci -A -m 'commit on test'
  adding b

  $ hg up default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ echo c > c
  $ hg ci -A -m 'commit on default'
  adding c
  $ hg up test
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark test)
  $ echo c > c
  $ hg ci -A -m 'commit on test'
  adding c

#if false
# Test that all log names are translated (e.g. branches, bookmarks):

  $ hg bookmark babar -r tip

  $ 'HGENCODING=UTF-8' 'LANGUAGE=de' hg log -r tip
  \xc3\x84nderung:        3:91f0fa364897 (esc)
  Lesezeichen:     babar
  Lesezeichen:     test
  Marke:           tip
  Vorg\xc3\xa4nger:       1:45efe61fb969 (esc)
  Nutzer:          test
  Datum:           Thu Jan 01 00:00:00 1970 +0000
  Zusammenfassung: commit on test
  $ hg bookmark -d babar
#endif

# log -p --cwd dir (in subdir)

  $ mkdir dir
  $ hg log -p --cwd dir
  commit:      * (glob)
  bookmark:    test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r * -r * c (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r * -r * c (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r * -r * a (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a

# log -p -R repo

  $ cd dir
  $ hg log -p -R .. ../a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r * -r * a (glob)
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
  $ hg ci -A -m 'init, unrelated'
  adding init
  $ echo foo > init
  $ hg ci -m 'change, unrelated'
  $ echo foo > foo
  $ hg ci -A -m 'add unrelated old foo'
  adding foo
  $ hg rm foo
  $ hg ci -m 'delete foo, unrelated'
  $ echo related > foo
  $ hg ci -A -m 'add foo, related'
  adding foo

  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch branch
  $ hg ci -A -m 'first branch, unrelated'
  adding branch
  $ touch foo
  $ hg ci -A -m 'create foo, related'
  adding foo
  $ echo change > foo
  $ hg ci -m 'change foo, related'

  $ hg up 6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'change foo in branch' > foo
  $ hg ci -m 'change foo in branch, related'
  $ hg merge 7
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ echo 'merge 1' > foo
  $ hg resolve -m foo
  (no more unresolved files)
  $ hg ci -m 'First merge, related'

  $ hg merge 4
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ echo 'merge 2' > foo
  $ hg resolve -m foo
  (no more unresolved files)
  $ hg ci -m 'Last merge, related'

  $ hg log --graph
  @    commit:      * (glob)
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     Last merge, related
  │ │
  │ o    commit:      * (glob)
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │ │  summary:     First merge, related
  │ │ │
  │ │ o  commit:      * (glob)
  │ │ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │ │  summary:     change foo in branch, related
  │ │ │
  │ o │  commit:      * (glob)
  │ ├─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:00 1970 +0000
  │ │    summary:     change foo, related
  │ │
  │ o  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     create foo, related
  │ │
  │ o  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     first branch, unrelated
  │ │
  o │  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     add foo, related
  │ │
  o │  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     delete foo, unrelated
  │ │
  o │  commit:      * (glob)
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     add unrelated old foo
  │ │
  o │  commit:      * (glob)
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     change, unrelated
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     init, unrelated

  $ hg --traceback log -f foo
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Last merge, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First merge, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo in branch, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     create foo, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related

# Also check when maxrev < lastrevfilelog

  $ hg --traceback log -f -r4 foo
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add unrelated old foo
  $ cd ..

# Issue2383: hg log showing _less_ differences than hg diff

  $ hg init issue2383
  $ cd issue2383

# Create a test repo:

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

# Merge:

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

# Make sure there's a file listed in the merge to trigger the bug:

  $ echo c > a
  $ hg ci -m3

# Two files shown here in diff:

  $ hg diff --rev '2:3'
  diff -r * -r * a (glob)
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b

# Diff here should be the same:

  $ hg log -vpr 3
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  3
  
  
  diff -r * -r * a (glob)
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r * -r * b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  $ cd ..

# 'hg log -r rev fn' when last(filelog(fn)) != rev

  $ hg init simplelog
  $ cd simplelog
  $ echo f > a
  $ hg ci -Ama -d '0 0'
  adding a
  $ echo f >> a
  $ hg ci '-Ama bis' -d '1 0'

  $ hg log -r0 a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a

# enable obsolete to test hidden feature

  $ cat >> $HGRCPATH << 'EOF'
  > [experimental]
  > evolution.createmarkers=True
  > EOF

  $ hg log '--template={rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg debugobsolete a765632148dc55d38c35c4f247c618701886cb2f
  $ hg up null -q
  $ hg log '--template={rev}:{node}\n'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg log '--template={rev}:{node}\n' --hidden
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg log -r a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a bis

# test that parent prevent a changeset to be hidden

  $ hg up 1 -q --hidden
  $ hg log '--template={rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

# test that second parent prevent a changeset to be hidden too

  $ hg debugsetparents 0 1
  $ hg log '--template={rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg debugsetparents 1
  $ hg up -q null

# bookmarks prevent a changeset being hidden

  $ hg bookmark --hidden -r 1 X
  $ hg log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg bookmark -d X

# divergent bookmarks are not hidden

  $ hg bookmark --hidden -r 1 'X@foo'
  $ hg log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

# test hidden revision 0 (issue5385)

  $ hg bookmark -d 'X@foo'
  $ hg up null -q
  $ hg debugobsolete 9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ echo f > b
  $ hg ci -Amb -d '2 0'
  adding b
  $ echo f >> b
  $ hg ci '-mb bis' -d '3 0'
  $ hg log '-T{rev}:{node}\n'
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  2:94375ec45bddd2a824535fc04855bd058c926ec0

  $ hg log '-T{rev}:{node}\n' '-r:'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  1:a765632148dc55d38c35c4f247c618701886cb2f
  2:94375ec45bddd2a824535fc04855bd058c926ec0
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  $ hg log '-T{rev}:{node}\n' '-r:tip'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  1:a765632148dc55d38c35c4f247c618701886cb2f
  2:94375ec45bddd2a824535fc04855bd058c926ec0
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  $ hg log '-T{rev}:{node}\n' '-r:0'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ hg log '-T{rev}:{node}\n' -f
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  2:94375ec45bddd2a824535fc04855bd058c926ec0

# clear extensions configuration

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'obs=!' >> $HGRCPATH
  $ cd ..

# test hg log on non-existent files and on directories

  $ newrepo issue1340
  $ mkdir d1 D2 D3.i d4.hg d5.d .d6
  $ echo 1 > d1/f1
  $ echo 1 > D2/f1
  $ echo 1 > D3.i/f1
  $ echo 1 > d4.hg/f1
  $ echo 1 > d5.d/f1
  $ echo 1 > .d6/f1
  $ hg -q add .
  $ hg commit -m 'a bunch of weird directories'
  $ hg log -l1 d1/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 f1
  $ hg log -l1 . -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 ./ -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 d1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 D2 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 D2/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 D3.i -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 D3.i/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 d4.hg -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 d4.hg/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 d5.d -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 d5.d/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 .d6 -T '{node|short}'
  07c07884437f (no-eol)
  $ hg log -l1 .d6/f1 -T '{node|short}'
  07c07884437f (no-eol)

# issue3772: hg log -r :null showing revision 0 as well

  $ hg log -r ':null'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ hg log -r 'null:null'
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000

# working-directory revision requires special treatment
# clean:

  $ hg log -r 'wdir()' --debug
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  $ hg log -r 'wdir()' -p --stat
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000

# dirty:

  $ echo 2 >> d1/f1
  $ echo 2 > d1/f2
  $ hg add d1/f2
  $ hg remove .d6/f1
  $ hg status
  M d1/f1
  A d1/f2
  R .d6/f1

  $ hg log -r 'wdir()'
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  $ hg log -r 'wdir()' -q
  ffffffffffff

  $ hg log -r 'wdir()' --debug
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       d1/f1
  files+:      d1/f2
  files-:      .d6/f1
  extra:       branch=default
  $ hg log -r 'wdir()' -p --stat --git
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  
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
    "parents": ["*"] (glob)
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
    "parents": ["*"], (glob)
    "manifest": null,
    "extra": {"branch": "default"},
    "modified": ["d1/f1"],
    "added": ["d1/f2"],
    "removed": [".d6/f1"]
   }
  ]

  $ hg revert -aqC

# Check that adding an arbitrary name shows up in log automatically

  $ cat > ../names.py << 'EOF'
  > """A small extension to test adding arbitrary names to a repo"""
  > from __future__ import absolute_import
  > from edenscm import namespaces, registrar
  > 
  > 
  > namespacepredicate = registrar.namespacepredicate()
  > 
  > @namespacepredicate("bars", priority=70)
  > def barlookup(repo):
  >     foo = {'foo': repo[0].node()}
  >     names = lambda r: foo.keys()
  >     namemap = lambda r, name: foo.get(name)
  >     nodemap = lambda r, node: [name for name, n in foo.items()
  >                                if n == node]
  >     return namespaces.namespace(
  >         templatename="bar",
  >         logname="barlog",
  >         colorname="barcolor",
  >         listnames=names,
  >         namemap=namemap,
  >         nodemap=nodemap
  >     )
  > EOF

  $ hg --config 'extensions.names=../names.py' log -r 0
  commit:      * (glob)
  barlog:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ hg --config 'extensions.names=../names.py' --config 'extensions.color=' --config 'color.log.barcolor=red' '--color=always' log -r 0
  \x1b[0m\x1b[1m\x1b[93mcommit:      07c07884437f\x1b[0m (esc)
  \x1b[31mbarlog:      foo\x1b[39m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ hg --config 'extensions.names=../names.py' log -r 0 --template '{bars}\n'
  foo

# revert side effect of names.py

  >>> from edenscm import namespaces
  >>> del namespaces.namespacetable["bars"]

# Templater parse errors:
# simple error

  $ hg log -r . -T '{shortest(node}'
  hg: parse error at 15: unexpected token: end
  ({shortest(node}
                 ^ here)
  [255]

# multi-line template with error

  $ hg log -r . -T 'line 1\nline2\n{shortest(node}\nline4\nline5'
  hg: parse error at 30: unexpected token: end
  (line 1\nline2\n{shortest(node}\nline4\nline5
                                ^ here)
  [255]

  $ cd ..

# hg log -f dir across branches

  $ hg init acrossbranches
  $ cd acrossbranches
  $ mkdir d
  $ echo a > d/a
  $ hg ci -Aqm a
  $ echo b > d/a
  $ hg ci -Aqm b
  $ hg up -q 0
  $ echo b > d/a
  $ hg ci -Aqm c
  $ hg log -f d -T '{desc}' -G
  @  c
  │
  o  a
  $ hg log -f d -T '{desc}' -G
  @  c
  │
  o  a
  $ hg log -f d/a -T '{desc}' -G
  @  c
  │
  o  a
  $ cd ..

# hg log -f with linkrev pointing to another branch
# -------------------------------------------------
# create history with a filerev whose linkrev points to another branch

  $ hg init branchedlinkrev
  $ cd branchedlinkrev
  $ echo 1 > a
  $ hg commit -Am content1
  adding a
  $ echo 2 > a
  $ hg commit -m content2
  $ hg up --rev 'desc(content1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo unrelated > unrelated
  $ hg commit -Am unrelated
  adding unrelated
  $ hg graft -r 'desc(content2)'
  grafting 2294ae80ad84 "content2"
  $ echo 3 > a
  $ hg commit -m content3
  $ hg log -G
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     content3
  │
  o  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     content2
  │
  o  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     unrelated
  │
  │ o  commit:      * (glob)
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     content2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# log -f on the file should list the graft result.

  $ hg log -Gf a
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     content3
  │
  o  commit:      * (glob)
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     content2
  ╷
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# plain log lists the original version
# (XXX we should probably list both)

  $ hg log -G a
  @  commit:      * (glob)
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     content3
  ╷
  ╷ o  commit:      * (glob)
  ╭─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     content2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# hg log -f from the grafted changeset
# (The bootstrap should properly take the topology in account)

  $ hg up 'desc(content3)^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -Gf a
  @  commit:      * (glob)
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     content2
  ╷
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# Test that we use the first non-hidden changeset in that case.
# (hide the changeset)

  $ hg log -T '{node}\n' -r 1
  2294ae80ad8447bc78383182eeac50cb049df623
  $ hg debugobsolete 2294ae80ad8447bc78383182eeac50cb049df623
  $ hg log -G
  o  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     content3
  │
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     content2
  │
  o  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     unrelated
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# Check that log on the file does not drop the file revision.

  $ hg log -G a
  o  commit:      * (glob)
  ╷  user:        test
  ╷  date:        Thu Jan 01 00:00:00 1970 +0000
  ╷  summary:     content3
  ╷
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

# Even when a head revision is linkrev-shadowed.

  $ hg log -T '{node}\n' -r 4
  50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ hg debugobsolete 50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ hg log -G a
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

  $ cd ..

# Even when the file revision is missing from some head:

  $ hg init issue4490
  $ cd issue4490
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'evolution.createmarkers=True' >> .hg/hgrc
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
  $ hg up 'head() and not .'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -G
  o  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     2
  │
  │ @  commit:      * (glob)
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     1
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0
  $ hg log -f -G b
  @  commit:      * (glob)
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  $ hg log -G b
  @  commit:      * (glob)
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  $ cd ..

# Check proper report when the manifest changes but not the file issue4499
# ------------------------------------------------------------------------

  $ hg init issue4499
  $ cd issue4499

  $ for f in A B C D E F G H I J K L M N O P Q R S T U; do
  >   echo 1 > $f
  > done

  $ hg add A B C D E F G H I J K L M N O P Q R S T U

  $ hg commit -m A1B1C1
  $ echo 2 > A
  $ echo 2 > B
  $ echo 2 > C
  $ hg commit -m A2B2C2
  $ hg up 0
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > A
  $ echo 2 > B
  $ echo 2 > C
  $ hg commit -m A3B2C2

  $ hg log -G
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     A3B2C2
  │
  │ o  commit:      * (glob)
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     A2B2C2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1

# Log -f on B should reports current changesets

  $ hg log -fG B
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     A3B2C2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1
  $ cd ..
