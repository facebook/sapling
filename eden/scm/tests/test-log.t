
#require no-eden

# coding=utf-8

# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
# isort:skip_file

# Log on empty repository: checking consistency

  $ setconfig devel.segmented-changelog-rev-compat=true

  $ sl init empty
  $ cd empty
  $ sl log
  $ sl log -r 1
  abort: unknown revision '1'!
  [255]
  $ sl log -r '-1:0'
  abort: unknown revision '-1'!
  [255]
  $ sl log -r 'branch(name)'
  $ sl log -r null -q
  000000000000

  $ cd ..

# The g is crafted to have 2 filelog topological heads in a linear
# changeset graph

  $ sl init a
  $ cd a
  $ echo a > a
  $ echo f > f
  $ sl ci -Ama -d '1 0'
  adding a
  adding f

  $ sl cp a b
  $ sl cp f g
  $ sl ci -mb -d '2 0'

  $ mkdir dir
  $ sl mv b dir
  $ echo g >> g
  $ echo f >> f
  $ sl ci -mc -d '3 0'

  $ sl mv a b
  $ sl cp -f f g
  $ echo a > d
  $ sl add d
  $ sl ci -md -d '4 0'

  $ sl mv dir/b e
  $ sl ci -me -d '5 0'

  $ sl --debug log a -T '{rev}: {desc}\n'
  0: a
  $ sl log a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  $ sl log 'glob:a*'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  $ sl --debug log 'glob:a*' -T '{rev}: {desc}\n'
  3: d
  0: a

# log on directory

  $ sl log dir
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  $ sl log somethingthatdoesntexist dir
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c

# -f, non-existent directory

  $ sl log -f dir
  abort: cannot follow file not in parent revision: "dir"
  [255]

# -f, directory
# (The code path using "follow()" revset will follow file renames, so 'b' and 'a' show up)

  $ sl up -q 'desc(d)'
  $ sl log -f dir
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

  $ sl log -f dir -p
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

  $ sl log -f -I 'dir**' -p
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r * -r * dir/b (glob)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  $ sl up -q 'desc(e)'

# -f, a wrong style

  $ sl log -f -l1 --style something
  abort: style 'something' not found
  (available styles: bisect, changelog, compact, default, phases, show, sl_default, status, xml)
  [255]

# -f, phases style

  $ sl log -f -l1 --style phases
  commit:      * (glob)
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e

  $ sl log -f -l1 --style phases -q
  * (glob)

# -f, but no args

  $ sl log -f
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

  $ sl up -q 'desc(c)'
  $ sl log -vf a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# many renames

  $ sl up -q tip
  $ sl log -vf e
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

  $ sl up -q 'desc(d)'
  $ sl log -pf dir/b
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

  $ sl '--cwd=dir' log -pf b
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

  $ sl log -pf
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

  $ sl log -vf dir/b
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

  $ sl up -q 'desc(c)'
  $ sl log -f g --template '{rev}\n'
  2
  1
  0
  $ sl up -q tip
  $ sl log -f g --template '{rev}\n'
  3
  2
  0

# log copies with --copies

  $ sl log -vC --template '{rev} {file_copies}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 

# log copies switch without --copies, with old filecopy template

  $ sl log -v --template '{rev} {file_copies_switch%filecopy}\n'
  4 
  3 
  2 
  1 
  0 

# log copies switch with --copies

  $ sl log -vC --template '{rev} {file_copies_switch}\n'
  4 e (dir/b)
  3 b (a)g (f)
  2 dir/b (b)
  1 b (a)g (f)
  0 

# log copies with hardcoded style and with --style=default

  $ sl log -vC -r4
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  $ sl log -vC -r4 '--style=default'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  $ sl log -vC -r4 -Tjson
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

  $ sl up -C 'desc(d)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl mv dir/b e
  $ echo foo > foo
  $ sl ci -Ame2 -d '6 0'
  adding foo
  $ sl log -v --template '{rev} {file_copies}\n' -r 5
  5 e (dir/b)

#if execbit
  $ chmod +x e
  $ sl ci -me3 -d '7 0'
  $ sl log -v --template '{rev} {file_copies}\n' -r 6
  6 
#endif

# log -p d

  $ sl log -pv d
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

  $ sl log --removed -v a
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

  $ sl log --removed -v '-r0:2' a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a f
  description:
  a

# log --removed with --limit should not keep scanning after limit is satisfied

  $ echo limit-target > limit-target
  $ sl ci -Aqm limit-target
  $ CODING_AGENT_METADATA=id=test_agent sl --config agent.max-commit-fetch-count=4 --config experimental.commit-fetch-batch-size=2 --config experimental.pathhistory=false log --removed -l 1 limit-target -T '{desc}\n'
  limit-target
  $ cd ..

# log --follow tests

  $ sl init follow
  $ cd follow

  $ echo base > base
  $ sl ci -Ambase -d '1 0'
  adding base

  $ echo r1 >> base
  $ sl ci -Amr1 -d '1 0'
  $ echo r2 >> base
  $ sl ci -Amr2 -d '1 0'

  $ sl up -C 'desc(r1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b1 > b1

# log -r "follow('set:clean()')"

  $ sl log -r 'follow('\''set:clean()'\'')'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1

  $ sl ci -Amb1 -d '1 0'
  adding b1

# log -f

  $ sl log -f
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

  $ sl log -r 'follow('\''glob:b*'\'')'
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

  $ sl up -C 'desc(base)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b2 > b2
  $ sl ci -Amb2 -d '1 0'
  adding b2
  $ sl log -f -r '1 + 4'
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

  $ sl log -r 'follow('\''set:grep(b2)'\'')'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2

# log -r "follow('set:grep(b2)', 4)"

  $ sl up -qC 'desc(base)'
  $ sl log -r 'follow('\''set:grep(b2)'\'', 4)'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2

# follow files starting from multiple revisions:

  $ sl log -T '{rev}: {files}\n' -r 'follow('\''glob:b?'\'', startrev=2+3+4)'
  3: b1
  4: b2

# follow files starting from empty revision:

  $ sl log -T '{rev}: {files}\n' -r 'follow('\''glob:*'\'', startrev=.-.)'

# follow starting from revisions:

  $ sl log -Gq -r 'follow(startrev=2+4)'
  o  ddb82e70d1a1
  │
  │ o  60c670bf5b30
  │ │
  │ o  3d5bf5654eda
  ├─╯
  @  67e992f2c4f3

# follow the current revision:

  $ sl log -Gq -r 'follow()'
  @  67e992f2c4f3

  $ sl up -qC 'desc(b2)'

# log -f -r null

  $ sl log -f -r null
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  $ sl log -f -r null -G
  o  commit:      000000000000
     user:
     date:        Thu Jan 01 00:00:00 1970 +0000

# log -f with null parent

  $ sl up -C null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ sl log -f

# log -r .  with two parents

  $ sl up -C 'desc(b1)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl merge tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl log -r .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1

# log -r .  with one parent

  $ sl ci -mm12 -d '1 0'
  $ sl log -r .
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12

  $ echo postm >> b1
  $ sl ci -Amb1.1 '-d1 0'

# log --follow-first

  $ sl log --follow-first
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
  
  commit:      3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  commit:      67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base

# log -P 2

  $ sl log -P 2
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  commit:      ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1

# log -r tip -p --git

  $ sl log -r tip -p --git
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

  $ sl log -r ''
  sl: parse error: empty query
  [255]

# log -r <some unknown node id>

  $ sl log -r 1000000000000000000000000000000000000000
  abort: unknown revision '1000000000000000000000000000000000000000'!
  [255]

# log -k r1

  $ sl log -k r1
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1

# log -p -l2 --color=always

  $ sl --config 'extensions.color=' --config 'color.mode=ansi' log -p -l2 '--color=always'
  [0m[1m[93mcommit:      2404bbcab562[0m
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  [0m[1mdiff -r 302e9dd6890d -r 2404bbcab562 b1[0m
  [0m[1m[31m--- a/b1	Thu Jan 01 00:00:01 1970 +0000[0m
  \x1b[0m\x1b[1m\x1b[32m+++ b/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  [35m@@ -1,1 +1,2 @@[39m
   b1
  [92m+postm[39m
  
  [0m[1m[93mcommit:      302e9dd6890d[0m
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  [0m[1mdiff -r e62f78d544b4 -r 302e9dd6890d b2[0m
  \x1b[0m\x1b[1m\x1b[31m--- /dev/null	Thu Jan 01 00:00:00 1970 +0000\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32m+++ b/b2	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[35m@@ -0,0 +1,1 @@\x1b[39m (esc)
  \x1b[92m+b2\x1b[39m (esc)

# log -r tip --stat

  $ sl log -r tip --stat
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
   b1 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ cd ..

# log --follow --patch FILE in repository where linkrev isn't trustworthy
# (issue5376)

  $ sl init follow-dup
  $ cd follow-dup
  $ cat >> .sl/config << 'EOF'
  > [ui]
  > logtemplate = '=== {rev}: {desc}\n'
  > [diff]
  > nodates = True
  > EOF
  $ echo 0 >> a
  $ sl ci -qAm a0
  $ echo 1 >> a
  $ sl ci -m a1
  $ sl up -q 'desc(a0)'
  $ echo 1 >> a
  $ touch b
  $ sl ci -qAm 'a1 with b'
  $ echo 3 >> a
  $ sl ci -m a3

#  fctx.rev() == 2, but fctx.linkrev() == 1

  $ sl log -pf a
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

  $ sl up -q 'desc("a1 with b")'
  $ sl log -pf a
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

  $ sl init follow-multi
  $ cd follow-multi
  $ echo 0 >> a
  $ sl ci -qAm a
  $ sl cp a b
  $ sl ci -m 'a->b'
  $ echo 2 >> a
  $ sl ci -m a
  $ echo 3 >> b
  $ sl ci -m b
  $ echo 4 >> a
  $ echo 4 >> b
  $ sl ci -m 'a,b'
  $ echo 5 >> a
  $ sl ci -m a0
  $ echo 6 >> b
  $ sl ci -m b0
  $ sl up -q "desc('a,b')"
  $ echo 7 >> b
  $ sl ci -m b1
  $ echo 8 >> a
  $ sl ci -m a1
  $ sl rm a
  $ sl mv b a
  $ sl ci -m 'b1->a1'
  $ sl merge -qt ':local'
  $ sl ci -m '(a0,b1->a1)->a'

  $ sl log -GT '{rev}: {desc}\n'
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

  $ sl log -T '{rev}: {desc}\n' -f a
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

  $ sl init revorder
  $ cd revorder

  $ sl book -q b0
  $ echo 0 >> f0
  $ sl ci -qAm k0 -u u0
  $ sl book -q b1
  $ echo 1 >> f1
  $ sl ci -qAm k1 -u u1
  $ sl book -q b2
  $ echo 2 >> f2
  $ sl ci -qAm k2 -u u2

  $ sl goto -q b2
  $ echo 3 >> f2
  $ sl ci -qAm k2 -u u2
  $ sl goto -q b1
  $ echo 4 >> f1
  $ sl ci -qAm k1 -u u1
  $ sl goto -q b0
  $ echo 5 >> f0
  $ sl ci -qAm k0 -u u0

#  summary of revisions:

  $ sl log -G -T '{rev} {bookmarks} {author} {desc} {files}\n'
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

  $ sl log '-r::head()' -T '{rev} {author}\n' -u u0 -u u2
  0 u0
  2 u2
  3 u2
  5 u0
  $ sl log '-r::head()' -T '{rev} {author}\n' -u u2 -u u0
  0 u0
  2 u2
  3 u2
  5 u0

#  log -k TEXT in descending order, against compound set:

  $ sl log '-r5 + reverse(::3)' -T '{rev} {desc}\n' -k k0 -k k1 -k k2
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0
  $ sl log '-r5 + reverse(::3)' -T '{rev} {desc}\n' -k k2 -k k1 -k k0
  5 k0
  3 k2
  2 k2
  1 k1
  0 k0

#  log FILE in ascending order, against dagrange:

  $ sl log '-r1::' -T '{rev} {files}\n' f1 f2
  1 f1
  2 f2
  3 f2
  4 f1
  $ sl log '-r1::' -T '{rev} {files}\n' f2 f1
  1 f1
  2 f2
  3 f2
  4 f1

  $ cd ..

# User

  $ sl init usertest
  $ cd usertest

  $ echo a > a
  $ sl ci -A -m a -u 'User One <user1@example.org>'
  adding a
  $ echo b > b
  $ sl ci -A -m b -u 'User Two <user2@example.org>'
  adding b

  $ sl log -u 'User One <user1@example.org>'
  commit:      * (glob)
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  $ sl log -u user1 -u user2
  commit:      * (glob)
  user:        User Two <user2@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  commit:      * (glob)
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  $ sl log -u user3

  $ cd ..

  $ sl init branches
  $ cd branches

  $ echo a > a
  $ sl ci -A -m 'commit on default'
  adding a
  $ sl book test
  $ echo b > b
  $ sl ci -A -m 'commit on test'
  adding b

  $ sl up default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark test)
  $ echo c > c
  $ sl ci -A -m 'commit on default'
  adding c
  $ sl up test
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark test)
  $ echo c > c
  $ sl ci -A -m 'commit on test'
  adding c

#if false
# Test that all log names are translated (e.g. branches, bookmarks):

  $ sl bookmark babar -r tip

  $ 'HGENCODING=UTF-8' 'LANGUAGE=de' sl log -r tip
  \xc3\x84nderung:        3:91f0fa364897 (esc)
  Lesezeichen:     babar
  Lesezeichen:     test
  Marke:           tip
  Vorg\xc3\xa4nger:       1:45efe61fb969 (esc)
  Nutzer:          test
  Datum:           Thu Jan 01 00:00:00 1970 +0000
  Zusammenfassung: commit on test
  $ sl bookmark -d babar
#endif

# log -p --cwd dir (in subdir)

  $ mkdir dir
  $ sl log -p --cwd dir
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
  $ sl log -p -R .. ../a
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

  $ sl init follow2
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
  $ sl ci -A -m 'init, unrelated'
  adding init
  $ echo foo > init
  $ sl ci -m 'change, unrelated'
  $ echo foo > foo
  $ sl ci -A -m 'add unrelated old foo'
  adding foo
  $ sl rm foo
  $ sl ci -m 'delete foo, unrelated'
  $ echo related > foo
  $ sl ci -A -m 'add foo, related'
  adding foo

  $ sl up 'desc("init, unrelated")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch branch
  $ sl ci -A -m 'first branch, unrelated'
  adding branch
  $ touch foo
  $ sl ci -A -m 'create foo, related'
  adding foo
  $ echo change > foo
  $ sl ci -m 'change foo, related'

  $ sl up 'desc("create foo, related")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'change foo in branch' > foo
  $ sl ci -m 'change foo in branch, related'
  $ sl merge "desc('change foo, related')"
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'sl resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ echo 'merge 1' > foo
  $ sl resolve -m foo
  (no more unresolved files)
  $ sl ci -m 'First merge, related'

  $ sl merge 4
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'sl resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ echo 'merge 2' > foo
  $ sl resolve -m foo
  (no more unresolved files)
  $ sl ci -m 'Last merge, related'

  $ sl log --graph
  @    commit:      4dae8563d2c5
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     Last merge, related
  │ │
  │ o    commit:      7b35701b003e
  │ ├─╮  user:        test
  │ │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │ │  summary:     First merge, related
  │ │ │
  │ │ o  commit:      e5416ad8a855
  │ │ │  user:        test
  │ │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │ │  summary:     change foo in branch, related
  │ │ │
  │ o │  commit:      87fe3144dcfa
  │ ├─╯  user:        test
  │ │    date:        Thu Jan 01 00:00:00 1970 +0000
  │ │    summary:     change foo, related
  │ │
  │ o  commit:      dc6c325fe5ee
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     create foo, related
  │ │
  │ o  commit:      73db34516eb9
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     first branch, unrelated
  │ │
  o │  commit:      88176d361b69
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     add foo, related
  │ │
  o │  commit:      dd78ae4afb56
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     delete foo, unrelated
  │ │
  o │  commit:      c4c64aedf0f7
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     add unrelated old foo
  │ │
  o │  commit:      e5faa7440653
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     change, unrelated
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     init, unrelated

  $ sl --traceback log -f foo
  commit:      4dae8563d2c5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Last merge, related
  
  commit:      7b35701b003e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First merge, related
  
  commit:      e5416ad8a855
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo in branch, related
  
  commit:      87fe3144dcfa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo, related
  
  commit:      dc6c325fe5ee
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     create foo, related
  
  commit:      88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related

# Also check when maxrev < lastrevfilelog

  $ sl --traceback log -f -r4 foo
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add unrelated old foo
  $ cd ..

# Issue2383: sl log showing _less_ differences than sl diff

  $ sl init issue2383
  $ cd issue2383

# Create a test repo:

  $ echo a > a
  $ sl ci -Am0
  adding a
  $ echo b > b
  $ sl ci -Am1
  adding b
  $ sl co 'desc(0)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > a
  $ sl ci -m2

# Merge:

  $ sl merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

# Make sure there's a file listed in the merge to trigger the bug:

  $ echo c > a
  $ sl ci -m3

# Two files shown here in diff:

  $ sl diff --rev '2:3'
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

  $ sl log -vpr 3
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

# 'sl log -r rev fn' when last(filelog(fn)) != rev

  $ sl init simplelog
  $ cd simplelog
  $ echo f > a
  $ sl ci -Ama -d '0 0'
  adding a
  $ echo f >> a
  $ sl ci '-Ama bis' -d '1 0'

  $ sl log -r0 a
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a

# compatibility with old tests

  $ sl debugobsolete a765632148dc55d38c35c4f247c618701886cb2f

# test that parent prevent a changeset to be hidden

  $ sl up 'desc("a bis")' -q --hidden
  $ sl log '--template={rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

# test that second parent prevent a changeset to be hidden too

  $ sl debugsetparents 0 1
  $ sl log '--template={rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ sl debugsetparents 1
  $ sl up -q null

# bookmarks prevent a changeset being hidden

  $ sl bookmark --hidden -r 1 X
  $ sl log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ sl bookmark -d X

# divergent bookmarks are not hidden

  $ sl bookmark --hidden -r 1 'X@foo'
  $ sl log --template '{rev}:{node}\n'
  1:a765632148dc55d38c35c4f247c618701886cb2f
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05

# test hidden revision 0 (issue5385)

  $ sl bookmark -d 'X@foo'
  $ sl up null -q
  $ sl debugobsolete 9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ echo f > b
  $ sl ci -Amb -d '2 0'
  adding b
  $ echo f >> b
  $ sl ci '-mb bis' -d '3 0'
  $ sl log '-T{rev}:{node}\n'
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  2:94375ec45bddd2a824535fc04855bd058c926ec0

  $ sl log '-T{rev}:{node}\n' '-r:'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  1:a765632148dc55d38c35c4f247c618701886cb2f
  2:94375ec45bddd2a824535fc04855bd058c926ec0
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  $ sl log '-T{rev}:{node}\n' '-r:tip'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  1:a765632148dc55d38c35c4f247c618701886cb2f
  2:94375ec45bddd2a824535fc04855bd058c926ec0
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  $ sl log '-T{rev}:{node}\n' '-r:0'
  0:9f758d63dcde62d547ebfb08e1e7ee96535f2b05
  $ sl log '-T{rev}:{node}\n' -f
  3:d7d28b288a6b83d5d2cf49f10c5974deed3a1d2e
  2:94375ec45bddd2a824535fc04855bd058c926ec0

# clear extensions configuration

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'obs=!' >> $HGRCPATH
  $ cd ..

# test sl log on non-existent files and on directories

  $ newrepo issue1340
  $ mkdir d1 D2 D3.i d4.hg d5.d .d6
  $ echo 1 > d1/f1
  $ echo 1 > D2/f1
  $ echo 1 > D3.i/f1
  $ echo 1 > d4.hg/f1
  $ echo 1 > d5.d/f1
  $ echo 1 > .d6/f1
  $ sl -q add .
  $ sl commit -m 'a bunch of weird directories'
  $ sl log -l1 d1/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 f1
  $ sl log -l1 . -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 ./ -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 d1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 D2 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 D2/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 D3.i -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 D3.i/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 d4.hg -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 d4.hg/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 d5.d -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 d5.d/f1 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 .d6 -T '{node|short}'
  07c07884437f (no-eol)
  $ sl log -l1 .d6/f1 -T '{node|short}'
  07c07884437f (no-eol)

# issue3772: sl log -r :null showing revision 0 as well

  $ sl log -r ':null'
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ sl log -r 'null:null'
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000

# working-directory revision requires special treatment
# clean:

  $ sl log -r 'wdir()' --debug
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  $ sl log -r 'wdir()' -p --stat
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000

# dirty:

  $ echo 2 >> d1/f1
  $ echo 2 > d1/f2
  $ sl add d1/f2
  $ sl remove .d6/f1
  $ sl status
  M d1/f1
  A d1/f2
  R .d6/f1

  $ sl log -r 'wdir()'
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  $ sl log -r 'wdir()' -q
  ffffffffffff

  $ sl log -r 'wdir()' --debug
  phase:       draft
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       d1/f1
  files+:      d1/f2
  files-:      .d6/f1
  $ sl log -r 'wdir()' -p --stat --git
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
  $ sl log -r 'wdir()' -Tjson
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

  $ sl log -r 'wdir()' -Tjson -q
  [
   {
    "rev": null,
    "node": null
   }
  ]

  $ sl log -r 'wdir()' -Tjson --debug
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
    "extra": {},
    "modified": ["d1/f1"],
    "added": ["d1/f2"],
    "removed": [".d6/f1"]
   }
  ]

  $ sl revert -aqC

# Check that adding an arbitrary name shows up in log automatically

  $ cat > ../names.py << 'EOF'
  > """A small extension to test adding arbitrary names to a repo"""
  > from __future__ import absolute_import
  > from sapling import namespaces, registrar
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

  $ sl --config 'extensions.names=../names.py' log -r 0
  commit:      * (glob)
  barlog:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ sl --config 'extensions.names=../names.py' --config 'extensions.color=' --config 'color.log.barcolor=red' '--color=always' log -r 0
  \x1b[0m\x1b[1m\x1b[93mcommit:      07c07884437f\x1b[0m (esc)
  \x1b[31mbarlog:      foo\x1b[39m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a bunch of weird directories
  $ sl --config 'extensions.names=../names.py' log -r 0 --template '{bars}\n'
  foo

# revert side effect of names.py

  >>> from sapling import namespaces
  >>> del namespaces.namespacetable["bars"]

# Templater parse errors:
# simple error

  $ sl log -r . -T '{shortest(node}'
  sl: parse error at 15: unexpected token: end
  ({shortest(node}
                 ^ here)
  [255]

# multi-line template with error

  $ sl log -r . -T 'line 1\nline2\n{shortest(node}\nline4\nline5'
  sl: parse error at 30: unexpected token: end
  (line 1\nline2\n{shortest(node}\nline4\nline5
                                ^ here)
  [255]

  $ cd ..

# sl log -f dir across branches

  $ sl init acrossbranches
  $ cd acrossbranches
  $ mkdir d
  $ echo a > d/a
  $ sl ci -Aqm a
  $ echo b > d/a
  $ sl ci -Aqm b
  $ sl up -q 'desc(a)'
  $ echo b > d/a
  $ sl ci -Aqm c
  $ sl log -f d -T '{desc}' -G
  @  c
  │
  o  a
  $ sl log -f d -T '{desc}' -G
  @  c
  │
  o  a
  $ sl log -f d/a -T '{desc}' -G
  @  c
  │
  o  a
  $ cd ..

# sl log -f with linkrev pointing to another branch
# -------------------------------------------------
# create history with a filerev whose linkrev points to another branch

  $ sl init branchedlinkrev
  $ cd branchedlinkrev
  $ echo 1 > a
  $ sl commit -Am content1
  adding a
  $ echo 2 > a
  $ sl commit -m content2
  $ sl up --rev 'desc(content1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo unrelated > unrelated
  $ sl commit -Am unrelated
  adding unrelated
  $ sl graft -r 'desc(content2)'
  grafting 2294ae80ad84 "content2"
  $ echo 3 > a
  $ sl commit -m content3
  $ sl log -G
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

  $ sl log -Gf a
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

  $ sl log -G a
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

# sl log -f from the grafted changeset
# (The bootstrap should properly take the topology in account)

  $ sl up 'desc(content3)^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl log -Gf a
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

  $ sl log -T '{node}\n' -r 1
  2294ae80ad8447bc78383182eeac50cb049df623
  $ sl debugobsolete 2294ae80ad8447bc78383182eeac50cb049df623
  $ sl log -G
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

  $ sl log -G a
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

  $ sl log -T '{node}\n' -r 4
  50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ sl debugobsolete 50b9b36e9c5df2c6fc6dcefa8ad0da929e84aed2
  $ sl log -G a
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     content1

  $ cd ..

# Even when the file revision is missing from some head:

  $ sl init issue4490
  $ cd issue4490
  $ echo '[experimental]' >> .sl/config
  $ echo 'evolution.createmarkers=True' >> .sl/config
  $ echo a > a
  $ sl ci -Am0
  adding a
  $ echo b > b
  $ sl ci -Am1
  adding b
  $ echo B > b
  $ sl ci --amend -m 1
  $ sl up 'desc(0)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ sl ci -Am2
  adding c
  $ sl up 'head() and not .'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl log -G
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
  $ sl log -f -G b
  @  commit:      * (glob)
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  $ sl log -G b
  @  commit:      * (glob)
  │  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1
  $ cd ..

# Check proper report when the manifest changes but not the file issue4499
# ------------------------------------------------------------------------

  $ sl init issue4499
  $ cd issue4499

  $ for f in A B C D E F G H I J K L M N O P Q R S T U; do
  >   echo 1 > $f
  > done

  $ sl add A B C D E F G H I J K L M N O P Q R S T U

  $ sl commit -m A1B1C1
  $ echo 2 > A
  $ echo 2 > B
  $ echo 2 > C
  $ sl commit -m A2B2C2
  $ sl up 'desc(A1B1C1)'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 3 > A
  $ echo 2 > B
  $ echo 2 > C
  $ sl commit -m A3B2C2

  $ sl log -G
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     A3B2C2
  │
  │ o  commit:      07dcc6b312c0
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     A2B2C2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1

# Log -f on B should reports current changesets

  $ sl log -fG B
  @  commit:      fe5fc3d0eb17
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     A3B2C2
  │
  o  commit:      * (glob)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A1B1C1
  $ cd ..
