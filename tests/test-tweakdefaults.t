  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > rebase=
  > EOF

Setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ mkdir dir
  $ touch dir/b
  $ hg commit -Aqm b
  $ hg up -q 0
  $ echo x >> a
  $ hg commit -Aqm a2

Empty update fails

  $ hg up -q 0
  $ hg up
  abort: You must specify a destination to update to, for example "hg update master".
  (If you're trying to move a bookmark forward, try "hg rebase -d <destination>".)
  [255]

  $ hg up -q -r 1
  $ hg log -r . -T '{rev}\n'
  1
  $ hg up -q 1
  $ hg log -r . -T '{rev}\n'
  1

Updating to a specific date isn't blocked by our extensions'

  $ hg bookmark temp
  $ hg up -d "<today"
  found revision 2 from Thu Jan 01 00:00:00 1970 +0000
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark temp)
  $ hg up temp
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark temp)
  $ hg bookmark --delete temp

Log is -f by default

  $ hg log -G -T '{rev} {desc}\n'
  @  1 b
  |
  o  0 a
  
  $ hg log -G -T '{rev} {desc}\n' --all
  o  2 a2
  |
  | @  1 b
  |/
  o  0 a
  
Dirty update to different rev fails by default

  $ echo x >> a
  $ hg st
  M a
  $ hg update .^
  abort: uncommitted changes
  [255]

Dirty update allowed to same rev and with --nocheck and --clean

  $ hg update .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg update --nocheck .^
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg update --clean 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Log on dir's works

  $ hg log -T '{rev} {desc}\n' dir
  1 b

  $ hg log -T '{rev} {desc}\n' -I 'dir/*'
  1 b

Empty rebase fails

  $ hg rebase
  abort: you must specify a destination (-d) for the rebase
  [255]
  $ hg rebase -d 2
  rebasing 1:7b4cb4e1674c "b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/7b4cb4e1674c-f22b5b1e-backup.hg (glob)

Rebase fast forwards bookmark

  $ hg book -r 1 mybook
  $ hg up -q mybook
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  1 a2 mybook
  |
  o  0 a
  
  $ hg rebase -d 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  2 b mybook
  |
  o  1 a2
  |
  o  0 a
  
Rebase works with hyphens

  $ hg book -r 1 hyphen-book
  $ hg book -r 2 hyphen-dest
  $ hg up -q hyphen-book
  $ hg log --all -G -T '{rev} {desc} {bookmarks}\n'
  o  2 b hyphen-dest mybook
  |
  @  1 a2 hyphen-book
  |
  o  0 a
  
  $ hg rebase -d hyphen-dest
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --all -G -T '{rev} {desc} {bookmarks}\n'
  @  2 b hyphen-book hyphen-dest mybook
  |
  o  1 a2
  |
  o  0 a
  
Grep options work

  $ mkdir -p dir1/subdir1
  $ echo str1f1 >> dir1/f1
  $ echo str1-v >> dir1/-v
  $ echo str1space >> 'dir1/file with space'
  $ echo str1sub >> dir1/subdir1/subf1
  $ hg add -q dir1

  $ hg grep x
  a:x
  $ hg grep -i X
  a:x
  $ hg grep -l x
  a
  $ hg grep -n x
  a:1:x
  $ hg grep -V ''
  [123]

Make sure grep works in subdirectories and with strange filenames
  $ cd dir1
  $ hg grep str1
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub
  $ hg grep str1 'relre:f[0-9]+'
  f1:str1f1
  subdir1/subf1:str1sub

Basic vs extended regular expressions
  $ hg grep 'str([0-9])'
  [123]
  $ hg grep 'str\([0-9]\)'
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub
  $ hg grep -F 'str[0-9]'
  [123]
  $ hg grep -E 'str([0-9])'
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub

Filesets
  $ hg grep str1 'set:added()'
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub

Crazy filenames
  $ hg grep str1 -- -v
  -v:str1-v
  $ hg grep str1 '*v'
  -v:str1-v
  $ hg grep str1 'file with space'
  file with space:str1space
  $ hg grep str1 '*with*'
  file with space:str1space
  $ hg grep str1 '*f1'
  f1:str1f1
  $ hg grep str1 subdir1
  subdir1/subf1:str1sub
  $ hg grep str1 '**/*f1'
  f1:str1f1
  subdir1/subf1:str1sub

Test tweaked branch command
  $ hg branch
  default
  $ hg branch foo
  abort: do not use branches; use bookmarks instead
  (use --new if you are certain you want a branch)
  [255]
  $ hg branch --new foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ hg branch -C
  reset working directory to branch default

Test that status is default relative
  $ mkdir foo
  $ cd foo
  $ hg status
  A ../-v
  A ../f1
  A ../file with space
  A ../subdir1/subf1
  $ hg status --root-relative
  A dir1/-v
  A dir1/f1
  A dir1/file with space
  A dir1/subdir1/subf1
  $ hg status .
  $ hg status ../subdir1
  A ../subdir1/subf1

Don't break automation
  $ HGPLAIN=1 hg status
  A dir1/-v
  A dir1/f1
  A dir1/file with space
  A dir1/subdir1/subf1
