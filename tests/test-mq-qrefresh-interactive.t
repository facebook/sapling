Create configuration

  $ echo "[ui]" >> $HGRCPATH
  $ echo "interactive=true" >> $HGRCPATH

help qrefresh (no record)

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg help qrefresh
  hg qrefresh [-I] [-X] [-e] [-m TEXT] [-l FILE] [-s] [FILE]...
  
  update the current patch
  
      If any file patterns are provided, the refreshed patch will contain only
      the modifications that match those patterns; the remaining modifications
      will remain in the working directory.
  
      If -s/--short is specified, files currently included in the patch will be
      refreshed just like matched files and remain in the patch.
  
      If -e/--edit is specified, Mercurial will start your configured editor for
      you to enter a message. In case qrefresh fails, you will find a backup of
      your message in ".hg/last-message.txt".
  
      hg add/remove/copy/rename work as usual, though you might want to use git-
      style patches (-g/--git or [diff] git=1) to track copies and renames. See
      the diffs help topic for more information on the git diff format.
  
      Returns 0 on success.
  
  options:
  
   -e --edit                edit commit message
   -g --git                 use git extended diff format
   -s --short               refresh only files already in the patch and
                            specified files
   -U --currentuser         add/update author field in patch with current user
   -u --user USER           add/update author field in patch with given user
   -D --currentdate         add/update date field in patch with current date
   -d --date DATE           add/update date field in patch with given date
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
  
  [+] marked option can be specified multiple times
  
  use "hg -v help qrefresh" to show the global options

help qrefresh (record)

  $ echo "record=" >> $HGRCPATH
  $ hg help qrefresh
  hg qrefresh [-I] [-X] [-e] [-m TEXT] [-l FILE] [-s] [FILE]...
  
  update the current patch
  
      If any file patterns are provided, the refreshed patch will contain only
      the modifications that match those patterns; the remaining modifications
      will remain in the working directory.
  
      If -s/--short is specified, files currently included in the patch will be
      refreshed just like matched files and remain in the patch.
  
      If -e/--edit is specified, Mercurial will start your configured editor for
      you to enter a message. In case qrefresh fails, you will find a backup of
      your message in ".hg/last-message.txt".
  
      hg add/remove/copy/rename work as usual, though you might want to use git-
      style patches (-g/--git or [diff] git=1) to track copies and renames. See
      the diffs help topic for more information on the git diff format.
  
      Returns 0 on success.
  
  options:
  
   -e --edit                edit commit message
   -g --git                 use git extended diff format
   -s --short               refresh only files already in the patch and
                            specified files
   -U --currentuser         add/update author field in patch with current user
   -u --user USER           add/update author field in patch with given user
   -D --currentdate         add/update date field in patch with current date
   -d --date DATE           add/update date field in patch with given date
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -m --message TEXT        use text as commit message
   -l --logfile FILE        read commit message from file
   -i --interactive         interactively select changes to refresh
  
  [+] marked option can be specified multiple times
  
  use "hg -v help qrefresh" to show the global options

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
  $ hg commit -m aaa
  $ hg qnew -d '0 0' patch

Changing files

  $ sed -e 's/2/2 2/;s/4/4 4/' 1.txt > 1.txt.new
  $ sed -e 's/b/b b/' 2.txt > 2.txt.new
  $ sed -e 's/hello world/hello world!/' dir/a.txt > dir/a.txt.new

  $ mv -f 1.txt.new 1.txt
  $ mv -f 2.txt.new 2.txt
  $ mv -f dir/a.txt.new dir/a.txt

Whole diff

  $ hg diff --nodates
  diff -r ed27675cb5df 1.txt
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
  diff -r ed27675cb5df 2.txt
  --- a/2.txt
  +++ b/2.txt
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  diff -r ed27675cb5df dir/a.txt
  --- a/dir/a.txt
  +++ b/dir/a.txt
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up

partial qrefresh

  $ hg qrefresh -i -d '0 0' <<EOF
  > y
  > y
  > n
  > y
  > y
  > n
  > EOF
  diff --git a/1.txt b/1.txt
  2 hunks, 2 lines changed
  examine changes to '1.txt'? [Ynesfdaq?] 
  @@ -1,3 +1,3 @@
   1
  -2
  +2 2
   3
  record change 1/4 to '1.txt'? [Ynesfdaq?] 
  @@ -3,3 +3,3 @@
   3
  -4
  +4 4
   5
  record change 2/4 to '1.txt'? [Ynesfdaq?] 
  diff --git a/2.txt b/2.txt
  1 hunks, 1 lines changed
  examine changes to '2.txt'? [Ynesfdaq?] 
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  record change 3/4 to '2.txt'? [Ynesfdaq?] 
  diff --git a/dir/a.txt b/dir/a.txt
  1 hunks, 1 lines changed
  examine changes to 'dir/a.txt'? [Ynesfdaq?] 

After partial qrefresh 'tip'

  $ hg tip -p
  changeset:   1:0738af1a8211
  tag:         patch
  tag:         qbase
  tag:         qtip
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     [mq]: patch
  
  diff -r 1fd39ab63a33 -r 0738af1a8211 1.txt
  --- a/1.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/1.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   1
  -2
  +2 2
   3
   4
   5
  diff -r 1fd39ab63a33 -r 0738af1a8211 2.txt
  --- a/2.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/2.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  
After partial qrefresh 'diff'

  $ hg diff --nodates
  diff -r 0738af1a8211 1.txt
  --- a/1.txt
  +++ b/1.txt
  @@ -1,5 +1,5 @@
   1
   2 2
   3
  -4
  +4 4
   5
  diff -r 0738af1a8211 dir/a.txt
  --- a/dir/a.txt
  +++ b/dir/a.txt
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up

qrefresh interactively everything else

  $ hg qrefresh -i -d '0 0' <<EOF
  > y
  > y
  > y
  > y
  > EOF
  diff --git a/1.txt b/1.txt
  1 hunks, 1 lines changed
  examine changes to '1.txt'? [Ynesfdaq?] 
  @@ -1,5 +1,5 @@
   1
   2 2
   3
  -4
  +4 4
   5
  record change 1/2 to '1.txt'? [Ynesfdaq?] 
  diff --git a/dir/a.txt b/dir/a.txt
  1 hunks, 1 lines changed
  examine changes to 'dir/a.txt'? [Ynesfdaq?] 
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up
  record change 2/2 to 'dir/a.txt'? [Ynesfdaq?] 

After final qrefresh 'tip'

  $ hg tip -p
  changeset:   1:2c3f66afeed9
  tag:         patch
  tag:         qbase
  tag:         qtip
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     [mq]: patch
  
  diff -r 1fd39ab63a33 -r 2c3f66afeed9 1.txt
  --- a/1.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/1.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   1
  -2
  +2 2
   3
  -4
  +4 4
   5
  diff -r 1fd39ab63a33 -r 2c3f66afeed9 2.txt
  --- a/2.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/2.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,5 +1,5 @@
   a
  -b
  +b b
   c
   d
   e
  diff -r 1fd39ab63a33 -r 2c3f66afeed9 dir/a.txt
  --- a/dir/a.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/a.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,4 +1,4 @@
  -hello world
  +hello world!
   
   someone
   up
  

After qrefresh 'diff'

  $ hg diff --nodates

  $ cd ..
