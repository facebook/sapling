  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cp $extpath/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > fbamend=$TESTTMP/fbamend.py
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

Empty rebase with nooprebase=True (default) succeeds

  $ hg rebase -s tip -d "tip^1"
  nothing to rebase

Empty rebase with nooprebase=False fails

  $ hg rebase --config 'tweakdefaults.nooprebase=False' -s tip -d "tip^1"
  nothing to rebase
  [1]

Rebase fast forwards bookmark

  $ hg book -r 1 mybook
  $ hg up -q mybook
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  @  1 a2 mybook
  |
  o  0 a
  
  $ hg rebase -d 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  devel-warn: bookmarks write with no wlock at: * (glob)

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
  devel-warn: bookmarks write with no wlock at: * (glob)

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
  $ hg branch foo --config tweakdefaults.allowbranch=false
  abort: new named branches are disabled in this repository
  [255]
  $ hg branch -C
  reset working directory to branch default

  $ hg branch --config tweakdefaults.allowbranch=false --config tweakdefaults.branchmessage='testing' foo
  abort: testing
  [255]
  $ hg branch --config tweakdefaults.allowbranch=false --new foo
  abort: new named branches are disabled in this repository
  [255]

  $ hg branch foo
  abort: do not use branches; use bookmarks instead
  (use --new if you are certain you want a branch)
  [255]
  $ hg branch --new foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ hg branches --config tweakdefaults.branchesmessage='testing'| head -n0
  testing

Test tweaked merge command
  $ hg merge | head -n1
  abort: no matching bookmark to merge - please merge with an explicit rev or bookmark
  (run 'hg heads' to see all heads)

  $ hg merge --config tweakdefaults.allowmerge=false
  abort: merging is not supported for this repository
  (use rebase instead)
  [255]

  $ hg merge --config tweakdefaults.mergemessage='testing' --config tweakdefaults.mergehint='hint' --config tweakdefaults.allowmerge=false
  abort: testing
  (hint)
  [255]

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

Test tweaked rollback command
  $ hg rollback --config tweakdefaults.allowrollback=false
  abort: the use of rollback is disabled
  [255]
  $ hg rollback --config tweakdefaults.allowrollback=false --config tweakdefaults.rollbackmessage='testing'
  abort: testing
  [255]
  $ hg rollback --config tweakdefaults.allowrollback=false --config tweakdefaults.rollbackmessage='testing' --config tweakdefaults.rollbackhint='hint'
  abort: testing
  (hint)
  [255]

Test tweaked tag command
  $ hg tag foo
  $ hg tag --config tweakdefaults.allowtags=false foo
  abort: new tags are disabled in this repository
  [255]
  $ hg tag --config tweakdefaults.allowtags=false --config tweakdefaults.tagmessage='testing' foo
  abort: testing
  [255]

  $ hg tags --config tweakdefaults.tagsmessage='testing' | head -n0
  testing

Test graft date when tweakdefaults.graftkeepdate is not set
  $ hg revert -a -q
  $ hg up -q 1
  $ hg graft -q 2
  $ hg log -T "{rev}\n" -d "yesterday to today"
  4

Test graft date when tweakdefaults.graftkeepdate is not set and --date is provided
  $ hg up -q 1
  $ hg graft -q 2 --date "1 1"
  $ hg log -l 1 -T "{date} {rev}\n"
  1.01 5

Test graft date when tweakdefaults.graftkeepdate is set
  $ hg up -q 1
  $ hg graft -q 5 --config tweakdefaults.graftkeepdate=True
  $ hg log -l 1 -T "{date} {rev}\n"
  1.01 6

Test amend date when tweakdefaults.amendkeepdate is not set
  $ hg up -q 1
  $ echo x > a
  $ hg commit -Aqm "commit for amend"
  $ echo x > a
  $ hg amend -q -m "amended message"
  $ hg log -T "{rev}\n" -d "yesterday to today"
  7

Test amend date when tweakdefaults.amendkeepdate is set
  $ touch new_file
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ hg commit -q --amend -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ hg log -l 1 -T "{date} {rev}\n"
  0.00 8

Test commit --amend date when tweakdefaults.amendkeepdate is set
  $ echo a >> new_file
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ hg commit -q --amend -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ hg log -l 1 -T "{date} {rev}\n"
  0.00 9

Test commit --amend date when tweakdefaults.amendkeepdate is not set and --date is provided
  $ echo xxx > a
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > a
  $ hg commit -q --amend -m "amended message" --date "1 1"
  $ hg log -l 1 -T "{date} {rev}\n"
  1.01 10

Test rebase date when tweakdefaults.rebasekeepdate is not set
  $ echo test_1 > rebase_dest
  $ hg commit --date "1 1" -Aqm "dest commit for rebase"
  $ hg bookmark rebase_dest_test_1
  $ hg up -q .^
  $ echo test_1 > rebase_source
  $ hg commit --date "1 1" -Aqm "source commit for rebase"
  $ hg bookmark rebase_source_test_1
  $ hg rebase -q -s rebase_source_test_1 -d rebase_dest_test_1
  $ hg log -l 1 -T "{rev}\n" -d "yesterday to today"
  12

Test rebase date when tweakdefaults.rebasekeepdate is set
  $ echo test_2 > rebase_dest
  $ hg commit -Aqm "dest commit for rebase"
  $ hg bookmark rebase_dest_test_2
  $ hg up -q .^
  $ echo test_2 > rebase_source
  $ hg commit -Aqm "source commit for rebase"
  $ hg bookmark rebase_source_test_2
  $ hg rebase -q -s rebase_source_test_2 -d rebase_dest_test_2 --config tweakdefaults.rebasekeepdate=True
  $ hg log -l 2 -T "{date} {rev}\n"
  0.00 14
  0.00 13

Test reuse message flag by taking message from previous commit
  $ cd ../..
  $ hg up -q hyphen-book
  $ touch afile
  $ hg add afile
  $ hg commit -M 2
  $ hg log --template {desc} -r .
  b (no-eol)
  $ echo 'canada rocks, eh?' > afile
  $ hg commit -M . -m 'this command will fail'
  abort: --reuse-message and --message are mutually exclusive
  [255]
  $ echo 'Super duper commit message' > ../commitmessagefile
  $ hg commit -M . -l ../commitmessagefile
  abort: --reuse-message and --logfile are mutually exclusive
  [255]
  $ hg commit -M thisrevsetdoesnotexist
  abort: unknown revision 'thisrevsetdoesnotexist'!
  [255]
  $ HGEDITOR=cat hg commit -M . -e
  b
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'foo'
  HG: bookmark 'hyphen-book'
  HG: changed afile

