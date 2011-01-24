Create configuration

  $ echo "[ui]" >> $HGRCPATH
  $ echo "interactive=true" >> $HGRCPATH
  $ echo "[extensions]"     >> $HGRCPATH
  $ echo "record="          >> $HGRCPATH

help (no mq, so no qrecord)

  $ hg help qrecord
  hg: unknown command 'qrecord'
  Mercurial Distributed SCM
  
  basic commands:
  
   add        add the specified files on the next commit
   annotate   show changeset information by line for each file
   clone      make a copy of an existing repository
   commit     commit the specified files or all outstanding changes
   diff       diff repository (or selected files)
   export     dump the header and diffs for one or more changesets
   forget     forget the specified files on the next commit
   init       create a new repository in the given directory
   log        show revision history of entire repository or files
   merge      merge working directory with another revision
   pull       pull changes from the specified source
   push       push changes to the specified destination
   remove     remove the specified files on the next commit
   serve      start stand-alone webserver
   status     show changed files in the working directory
   summary    summarize working directory state
   update     update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]

help (mq present)

  $ echo "mq="              >> $HGRCPATH
  $ hg help qrecord
  hg qrecord [OPTION]... PATCH [FILE]...
  
  interactively record a new patch
  
      See "hg help qnew" & "hg help record" for more information and usage.
  
  options:
  
   -e --edit                 edit commit message
   -g --git                  use git extended diff format
   -U --currentuser          add "From: <current user>" to patch
   -u --user USER            add "From: <USER>" to patch
   -D --currentdate          add "Date: <current date>" to patch
   -d --date DATE            add "Date: <DATE>" to patch
   -I --include PATTERN [+]  include names matching the given patterns
   -X --exclude PATTERN [+]  exclude names matching the given patterns
   -m --message TEXT         use text as commit message
   -l --logfile FILE         read commit message from file
  
  [+] marked option can be specified multiple times
  
  use "hg -v help qrecord" to show global options

  $ hg init a
  $ cd a

Base commit

  $ cat > 1.txt <<EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ cat > 2.txt <<EOF
  > a
  > b
  > c
  > d
  > e
  > f
  > EOF

  $ mkdir dir
  $ cat > dir/a.txt <<EOF
  > hello world
  > 
  > someone
  > up
  > there
  > loves
  > me
  > EOF

  $ hg add 1.txt 2.txt dir/a.txt
  $ hg commit -m 'initial checkin'

Changing files

  $ sed -e 's/2/2 2/;s/4/4 4/' 1.txt > 1.txt.new
  $ sed -e 's/b/b b/' 2.txt > 2.txt.new
  $ sed -e 's/hello world/hello world!/' dir/a.txt > dir/a.txt.new

  $ mv -f 1.txt.new 1.txt
  $ mv -f 2.txt.new 2.txt
  $ mv -f dir/a.txt.new dir/a.txt

Whole diff

  $ hg diff --nodates
  diff -r 1057167b20ef 1.txt
  --- a/1.txt
  +++ b/1.txt
  @@ -1,5 +1,5 @@
   1
  -2
  +2 2
   3
  -4
  +4 4
   5
  diff -r 1057167b20ef 2.txt
  --- a/2.txt
  +++ b/2.txt
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  diff -r 1057167b20ef dir/a.txt
  --- a/dir/a.txt
  +++ b/dir/a.txt
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up

qrecord a.patch

  $ hg qrecord -d '0 0' -m aaa a.patch <<EOF
  > y
  > y
  > n
  > y
  > y
  > n
  > EOF
  diff --git a/1.txt b/1.txt
  2 hunks, 2 lines changed
  examine changes to '1.txt'? [Ynsfdaq?] 
  @@ -1,3 +1,3 @@
   1
  -2
  +2 2
   3
  record change 1/4 to '1.txt'? [Ynsfdaq?] 
  @@ -3,3 +3,3 @@
   3
  -4
  +4 4
   5
  record change 2/4 to '1.txt'? [Ynsfdaq?] 
  diff --git a/2.txt b/2.txt
  1 hunks, 1 lines changed
  examine changes to '2.txt'? [Ynsfdaq?] 
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  record change 3/4 to '2.txt'? [Ynsfdaq?] 
  diff --git a/dir/a.txt b/dir/a.txt
  1 hunks, 1 lines changed
  examine changes to 'dir/a.txt'? [Ynsfdaq?] 

After qrecord a.patch 'tip'"

  $ hg tip -p
  changeset:   1:5d1ca63427ee
  tag:         a.patch
  tag:         qbase
  tag:         qtip
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     aaa
  
  diff -r 1057167b20ef -r 5d1ca63427ee 1.txt
  --- a/1.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/1.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   1
  -2
  +2 2
   3
   4
   5
  diff -r 1057167b20ef -r 5d1ca63427ee 2.txt
  --- a/2.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/2.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  

After qrecord a.patch 'diff'"

  $ hg diff --nodates
  diff -r 5d1ca63427ee 1.txt
  --- a/1.txt
  +++ b/1.txt
  @@ -1,5 +1,5 @@
   1
   2 2
   3
  -4
  +4 4
   5
  diff -r 5d1ca63427ee dir/a.txt
  --- a/dir/a.txt
  +++ b/dir/a.txt
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up

qrecord b.patch

  $ hg qrecord -d '0 0' -m bbb b.patch <<EOF
  > y
  > y
  > y
  > y
  > EOF
  diff --git a/1.txt b/1.txt
  1 hunks, 1 lines changed
  examine changes to '1.txt'? [Ynsfdaq?] 
  @@ -1,5 +1,5 @@
   1
   2 2
   3
  -4
  +4 4
   5
  record change 1/2 to '1.txt'? [Ynsfdaq?] 
  diff --git a/dir/a.txt b/dir/a.txt
  1 hunks, 1 lines changed
  examine changes to 'dir/a.txt'? [Ynsfdaq?] 
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up
  record change 2/2 to 'dir/a.txt'? [Ynsfdaq?] 

After qrecord b.patch 'tip'

  $ hg tip -p
  changeset:   2:b056198bf878
  tag:         b.patch
  tag:         qtip
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     bbb
  
  diff -r 5d1ca63427ee -r b056198bf878 1.txt
  --- a/1.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/1.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   1
   2 2
   3
  -4
  +4 4
   5
  diff -r 5d1ca63427ee -r b056198bf878 dir/a.txt
  --- a/dir/a.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/a.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up
  

After qrecord b.patch 'diff'

  $ hg diff --nodates
