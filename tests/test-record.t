Set up a repo

  $ echo "[ui]" >> $HGRCPATH
  $ echo "interactive=true" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "record=" >> $HGRCPATH

  $ hg init a
  $ cd a

Select no files

  $ touch empty-rw
  $ hg add empty-rw

  $ hg record empty-rw<<EOF
  > n
  > EOF
  diff --git a/empty-rw b/empty-rw
  new file mode 100644
  examine changes to 'empty-rw'? [Ynsfdaq?] 
  no changes to record

  $ hg tip -p
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  

Select files but no hunks

  $ hg record empty-rw<<EOF
  > y
  > n
  > EOF
  diff --git a/empty-rw b/empty-rw
  new file mode 100644
  examine changes to 'empty-rw'? [Ynsfdaq?] 
  abort: empty commit message
  [255]

  $ hg tip -p
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  
  

Record empty file

  $ hg record -d '0 0' -m empty empty-rw<<EOF
  > y
  > y
  > EOF
  diff --git a/empty-rw b/empty-rw
  new file mode 100644
  examine changes to 'empty-rw'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   0:c0708cf4e46e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     empty
  
  

Summary shows we updated to the new cset

  $ hg summary
  parent: 0:c0708cf4e46e tip
   empty
  branch: default
  commit: (clean)
  update: (current)

Rename empty file

  $ hg mv empty-rw empty-rename
  $ hg record -d '1 0' -m rename<<EOF
  > y
  > EOF
  diff --git a/empty-rw b/empty-rename
  rename from empty-rw
  rename to empty-rename
  examine changes to 'empty-rw' and 'empty-rename'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   1:d695e8dcb197
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     rename
  
  

Copy empty file

  $ hg cp empty-rename empty-copy
  $ hg record -d '2 0' -m copy<<EOF
  > y
  > EOF
  diff --git a/empty-rename b/empty-copy
  copy from empty-rename
  copy to empty-copy
  examine changes to 'empty-rename' and 'empty-copy'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   2:1d4b90bea524
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     copy
  
  

Delete empty file

  $ hg rm empty-copy
  $ hg record -d '3 0' -m delete<<EOF
  > y
  > EOF
  diff --git a/empty-copy b/empty-copy
  deleted file mode 100644
  examine changes to 'empty-copy'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   3:b39a238f01a1
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     delete
  
  

Add binary file

  $ hg bundle --base -2 tip.bundle
  1 changesets found
  $ hg add tip.bundle
  $ hg record -d '4 0' -m binary<<EOF
  > y
  > EOF
  diff --git a/tip.bundle b/tip.bundle
  new file mode 100644
  this is a binary file
  examine changes to 'tip.bundle'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   4:ad816da3711e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     binary
  
  diff -r b39a238f01a1 -r ad816da3711e tip.bundle
  Binary file tip.bundle has changed
  

Change binary file

  $ hg bundle --base -2 tip.bundle
  1 changesets found
  $ hg record -d '5 0' -m binary-change<<EOF
  > y
  > EOF
  diff --git a/tip.bundle b/tip.bundle
  this modifies a binary file (all or nothing)
  examine changes to 'tip.bundle'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   5:dccd6f3eb485
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     binary-change
  
  diff -r ad816da3711e -r dccd6f3eb485 tip.bundle
  Binary file tip.bundle has changed
  

Rename and change binary file

  $ hg mv tip.bundle top.bundle
  $ hg bundle --base -2 top.bundle
  1 changesets found
  $ hg record -d '6 0' -m binary-change-rename<<EOF
  > y
  > EOF
  diff --git a/tip.bundle b/top.bundle
  rename from tip.bundle
  rename to top.bundle
  this modifies a binary file (all or nothing)
  examine changes to 'tip.bundle' and 'top.bundle'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   6:7fa44105f5b3
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:06 1970 +0000
  summary:     binary-change-rename
  
  diff -r dccd6f3eb485 -r 7fa44105f5b3 tip.bundle
  Binary file tip.bundle has changed
  diff -r dccd6f3eb485 -r 7fa44105f5b3 top.bundle
  Binary file top.bundle has changed
  

Add plain file

  $ for i in 1 2 3 4 5 6 7 8 9 10; do
  >     echo $i >> plain
  > done

  $ hg add plain
  $ hg record -d '7 0' -m plain plain<<EOF
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  new file mode 100644
  examine changes to 'plain'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   7:11fb457c1be4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     plain
  
  diff -r 7fa44105f5b3 -r 11fb457c1be4 plain
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:07 1970 +0000
  @@ -0,0 +1,10 @@
  +1
  +2
  +3
  +4
  +5
  +6
  +7
  +8
  +9
  +10
  

Modify end of plain file

  $ echo 11 >> plain
  $ hg record -d '8 0' -m end plain <<EOF
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  1 hunks, 1 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -8,3 +8,4 @@
   8
   9
   10
  +11
  record this change to 'plain'? [Ynsfdaq?] 

Modify end of plain file, no EOL

  $ hg tip --template '{node}' >> plain
  $ hg record -d '9 0' -m noeol plain <<EOF
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  1 hunks, 1 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -9,3 +9,4 @@
   9
   10
   11
  +7264f99c5f5ff3261504828afa4fb4d406c3af54
  \ No newline at end of file
  record this change to 'plain'? [Ynsfdaq?] 

Modify end of plain file, add EOL

  $ echo >> plain
  $ echo 1 > plain2
  $ hg add plain2
  $ hg record -d '10 0' -m eol plain plain2 <<EOF
  > y
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  1 hunks, 1 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -9,4 +9,4 @@
   9
   10
   11
  -7264f99c5f5ff3261504828afa4fb4d406c3af54
  \ No newline at end of file
  +7264f99c5f5ff3261504828afa4fb4d406c3af54
  record change 1/2 to 'plain'? [Ynsfdaq?] 
  diff --git a/plain2 b/plain2
  new file mode 100644
  examine changes to 'plain2'? [Ynsfdaq?] 

Modify beginning, trim end, record both, add another file to test
changes numbering

  $ rm plain
  $ for i in 2 2 3 4 5 6 7 8 9 10; do
  >   echo $i >> plain
  > done
  $ echo 2 >> plain2

  $ hg record -d '10 0' -m begin-and-end plain plain2 <<EOF
  > y
  > y
  > y
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  2 hunks, 3 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -1,4 +1,4 @@
  -1
  +2
   2
   3
   4
  record change 1/3 to 'plain'? [Ynsfdaq?] 
  @@ -8,5 +8,3 @@
   8
   9
   10
  -11
  -7264f99c5f5ff3261504828afa4fb4d406c3af54
  record change 2/3 to 'plain'? [Ynsfdaq?] 
  diff --git a/plain2 b/plain2
  1 hunks, 1 lines changed
  examine changes to 'plain2'? [Ynsfdaq?] 
  @@ -1,1 +1,2 @@
   1
  +2
  record change 3/3 to 'plain2'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   11:21df83db12b8
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:10 1970 +0000
  summary:     begin-and-end
  
  diff -r ddb8b281c3ff -r 21df83db12b8 plain
  --- a/plain	Thu Jan 01 00:00:10 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:10 1970 +0000
  @@ -1,4 +1,4 @@
  -1
  +2
   2
   3
   4
  @@ -8,5 +8,3 @@
   8
   9
   10
  -11
  -7264f99c5f5ff3261504828afa4fb4d406c3af54
  diff -r ddb8b281c3ff -r 21df83db12b8 plain2
  --- a/plain2	Thu Jan 01 00:00:10 1970 +0000
  +++ b/plain2	Thu Jan 01 00:00:10 1970 +0000
  @@ -1,1 +1,2 @@
   1
  +2
  

Trim beginning, modify end

  $ rm plain
  > for i in 4 5 6 7 8 9 10.new; do
  >   echo $i >> plain
  > done

Record end

  $ hg record -d '11 0' -m end-only plain <<EOF
  > y
  > n
  > y
  > EOF
  diff --git a/plain b/plain
  2 hunks, 4 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -1,9 +1,6 @@
  -2
  -2
  -3
   4
   5
   6
   7
   8
   9
  record change 1/2 to 'plain'? [Ynsfdaq?] 
  @@ -4,7 +1,7 @@
   4
   5
   6
   7
   8
   9
  -10
  +10.new
  record change 2/2 to 'plain'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   12:99337501826f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:11 1970 +0000
  summary:     end-only
  
  diff -r 21df83db12b8 -r 99337501826f plain
  --- a/plain	Thu Jan 01 00:00:10 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:11 1970 +0000
  @@ -7,4 +7,4 @@
   7
   8
   9
  -10
  +10.new
  

Record beginning

  $ hg record -d '12 0' -m begin-only plain <<EOF
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  1 hunks, 3 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -1,6 +1,3 @@
  -2
  -2
  -3
   4
   5
   6
  record this change to 'plain'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   13:bbd45465d540
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:12 1970 +0000
  summary:     begin-only
  
  diff -r 99337501826f -r bbd45465d540 plain
  --- a/plain	Thu Jan 01 00:00:11 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:12 1970 +0000
  @@ -1,6 +1,3 @@
  -2
  -2
  -3
   4
   5
   6
  

Add to beginning, trim from end

  $ rm plain
  $ for i in 1 2 3 4 5 6 7 8 9; do
  >  echo $i >> plain
  > done

Record end

  $ hg record --traceback -d '13 0' -m end-again plain<<EOF
  > y
  > n
  > y
  > EOF
  diff --git a/plain b/plain
  2 hunks, 4 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -1,6 +1,9 @@
  +1
  +2
  +3
   4
   5
   6
   7
   8
   9
  record change 1/2 to 'plain'? [Ynsfdaq?] 
  @@ -1,7 +4,6 @@
   4
   5
   6
   7
   8
   9
  -10.new
  record change 2/2 to 'plain'? [Ynsfdaq?] 

Add to beginning, middle, end

  $ rm plain
  $ for i in 1 2 3 4 5 5.new 5.reallynew 6 7 8 9 10 11; do
  >   echo $i >> plain
  > done

Record beginning, middle

  $ hg record -d '14 0' -m middle-only plain <<EOF
  > y
  > y
  > y
  > n
  > EOF
  diff --git a/plain b/plain
  3 hunks, 7 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -1,2 +1,5 @@
  +1
  +2
  +3
   4
   5
  record change 1/3 to 'plain'? [Ynsfdaq?] 
  @@ -1,6 +4,8 @@
   4
   5
  +5.new
  +5.reallynew
   6
   7
   8
   9
  record change 2/3 to 'plain'? [Ynsfdaq?] 
  @@ -3,4 +8,6 @@
   6
   7
   8
   9
  +10
  +11
  record change 3/3 to 'plain'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   15:f34a7937ec33
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:14 1970 +0000
  summary:     middle-only
  
  diff -r 82c065d0b850 -r f34a7937ec33 plain
  --- a/plain	Thu Jan 01 00:00:13 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:14 1970 +0000
  @@ -1,5 +1,10 @@
  +1
  +2
  +3
   4
   5
  +5.new
  +5.reallynew
   6
   7
   8
  

Record end

  $ hg record -d '15 0' -m end-only plain <<EOF
  > y
  > y
  > EOF
  diff --git a/plain b/plain
  1 hunks, 2 lines changed
  examine changes to 'plain'? [Ynsfdaq?] 
  @@ -9,3 +9,5 @@
   7
   8
   9
  +10
  +11
  record this change to 'plain'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   16:f9900b71a04c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:15 1970 +0000
  summary:     end-only
  
  diff -r f34a7937ec33 -r f9900b71a04c plain
  --- a/plain	Thu Jan 01 00:00:14 1970 +0000
  +++ b/plain	Thu Jan 01 00:00:15 1970 +0000
  @@ -9,3 +9,5 @@
   7
   8
   9
  +10
  +11
  

  $ mkdir subdir
  $ cd subdir
  $ echo a > a
  $ hg ci -d '16 0' -Amsubdir
  adding subdir/a

  $ echo a >> a
  $ hg record -d '16 0' -m subdir-change a <<EOF
  > y
  > y
  > EOF
  diff --git a/subdir/a b/subdir/a
  1 hunks, 1 lines changed
  examine changes to 'subdir/a'? [Ynsfdaq?] 
  @@ -1,1 +1,2 @@
   a
  +a
  record this change to 'subdir/a'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   18:61be427a9deb
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:16 1970 +0000
  summary:     subdir-change
  
  diff -r a7ffae4d61cb -r 61be427a9deb subdir/a
  --- a/subdir/a	Thu Jan 01 00:00:16 1970 +0000
  +++ b/subdir/a	Thu Jan 01 00:00:16 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  

  $ echo a > f1
  $ echo b > f2
  $ hg add f1 f2

  $ hg ci -mz -d '17 0'

  $ echo a >> f1
  $ echo b >> f2

Help, quit

  $ hg record <<EOF
  > ?
  > q
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  y - record this change
  n - skip this change
  s - skip remaining changes to this file
  f - record remaining changes to this file
  d - done, skip remaining changes and files
  a - record all changes to all remaining files
  q - quit, recording no changes
  ? - display help
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  abort: user quit
  [255]

Skip

  $ hg record <<EOF
  > s
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  diff --git a/subdir/f2 b/subdir/f2
  1 hunks, 1 lines changed
  examine changes to 'subdir/f2'? [Ynsfdaq?] abort: response expected
  [255]

No

  $ hg record <<EOF
  > n
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  diff --git a/subdir/f2 b/subdir/f2
  1 hunks, 1 lines changed
  examine changes to 'subdir/f2'? [Ynsfdaq?] abort: response expected
  [255]

f, quit

  $ hg record <<EOF
  > f
  > q
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  diff --git a/subdir/f2 b/subdir/f2
  1 hunks, 1 lines changed
  examine changes to 'subdir/f2'? [Ynsfdaq?] 
  abort: user quit
  [255]

s, all

  $ hg record -d '18 0' -mx <<EOF
  > s
  > a
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  diff --git a/subdir/f2 b/subdir/f2
  1 hunks, 1 lines changed
  examine changes to 'subdir/f2'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   20:b3df3dda369a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:18 1970 +0000
  summary:     x
  
  diff -r 6e02d6c9906d -r b3df3dda369a subdir/f2
  --- a/subdir/f2	Thu Jan 01 00:00:17 1970 +0000
  +++ b/subdir/f2	Thu Jan 01 00:00:18 1970 +0000
  @@ -1,1 +1,2 @@
   b
  +b
  

f

  $ hg record -d '19 0' -my <<EOF
  > f
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   21:38ec577f126b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:19 1970 +0000
  summary:     y
  
  diff -r b3df3dda369a -r 38ec577f126b subdir/f1
  --- a/subdir/f1	Thu Jan 01 00:00:18 1970 +0000
  +++ b/subdir/f1	Thu Jan 01 00:00:19 1970 +0000
  @@ -1,1 +1,2 @@
   a
  +a
  

Preserve chmod +x

  $ chmod +x f1
  $ echo a >> f1
  $ hg record -d '20 0' -mz <<EOF
  > y
  > y
  > y
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  old mode 100644
  new mode 100755
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  @@ -1,2 +1,3 @@
   a
   a
  +a
  record this change to 'subdir/f1'? [Ynsfdaq?] 

  $ hg tip --config diff.git=True -p
  changeset:   22:3261adceb075
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:20 1970 +0000
  summary:     z
  
  diff --git a/subdir/f1 b/subdir/f1
  old mode 100644
  new mode 100755
  --- a/subdir/f1
  +++ b/subdir/f1
  @@ -1,2 +1,3 @@
   a
   a
  +a
  

Preserve execute permission on original

  $ echo b >> f1
  $ hg record -d '21 0' -maa <<EOF
  > y
  > y
  > y
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  @@ -1,3 +1,4 @@
   a
   a
   a
  +b
  record this change to 'subdir/f1'? [Ynsfdaq?] 

  $ hg tip --config diff.git=True -p
  changeset:   23:b429867550db
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:21 1970 +0000
  summary:     aa
  
  diff --git a/subdir/f1 b/subdir/f1
  --- a/subdir/f1
  +++ b/subdir/f1
  @@ -1,3 +1,4 @@
   a
   a
   a
  +b
  

Preserve chmod -x

  $ chmod -x f1
  $ echo c >> f1
  $ hg record -d '22 0' -mab <<EOF
  > y
  > y
  > y
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  old mode 100755
  new mode 100644
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  @@ -2,3 +2,4 @@
   a
   a
   b
  +c
  record this change to 'subdir/f1'? [Ynsfdaq?] 

  $ hg tip --config diff.git=True -p
  changeset:   24:0b082130c20a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:22 1970 +0000
  summary:     ab
  
  diff --git a/subdir/f1 b/subdir/f1
  old mode 100755
  new mode 100644
  --- a/subdir/f1
  +++ b/subdir/f1
  @@ -2,3 +2,4 @@
   a
   a
   b
  +c
  

  $ cd ..

Abort early when a merge is in progress

  $ hg up 4
  1 files updated, 0 files merged, 6 files removed, 0 files unresolved

  $ touch iwillmergethat
  $ hg add iwillmergethat

  $ hg branch thatbranch
  marked working directory as branch thatbranch

  $ hg ci -m'new head'

  $ hg up default
  6 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ hg merge thatbranch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg record -m'will abort'
  abort: cannot partially commit a merge (use "hg commit" instead)
  [255]

  $ hg up -C
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

With win32text

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'win32text = ' >> .hg/hgrc
  $ echo '[decode]' >> .hg/hgrc
  $ echo '** = cleverdecode:' >> .hg/hgrc
  $ echo '[encode]' >> .hg/hgrc
  $ echo '** = cleverencode:' >> .hg/hgrc
  $ echo '[patch]' >> .hg/hgrc
  $ echo 'eol = crlf' >> .hg/hgrc

Ignore win32text deprecation warning for now:

  $ echo '[win32text]' >> .hg/hgrc
  $ echo 'warn = no' >> .hg/hgrc

  $ echo d >> subdir/f1
  $ hg record -d '23 0' -mw1 <<EOF
  > y
  > y
  > EOF
  diff --git a/subdir/f1 b/subdir/f1
  1 hunks, 1 lines changed
  examine changes to 'subdir/f1'? [Ynsfdaq?] 
  @@ -3,3 +3,4 @@
   a
   b
   c
  +d
  record this change to 'subdir/f1'? [Ynsfdaq?] 

  $ hg tip -p
  changeset:   26:b8306e70edc4
  tag:         tip
  parent:      24:0b082130c20a
  user:        test
  date:        Thu Jan 01 00:00:23 1970 +0000
  summary:     w1
  
  diff -r 0b082130c20a -r b8306e70edc4 subdir/f1
  --- a/subdir/f1	Thu Jan 01 00:00:22 1970 +0000
  +++ b/subdir/f1	Thu Jan 01 00:00:23 1970 +0000
  @@ -3,3 +3,4 @@
   a
   b
   c
  +d
  
