#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable amend histedit rebase tweakdefaults
  $ setconfig experimental.updatecheck=noconflict
  $ setconfig ui.suggesthgprev=True

Setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg commit -Aqm a
  $ mkdir dir
  $ touch dir/b
  $ hg commit -Aqm b
  $ hg up -q 'desc(a)'
  $ echo x >> a
  $ hg commit -Aqm a2
  $ hg up -q 'desc(b)'

Updating to a specific date isn't blocked by our extensions'

  $ hg bookmark temp
  $ hg up -d "<today"
  found revision ae5108b653e2f2d15099970dec82ee0198e23d98 from Thu Jan 01 00:00:00 1970 +0000
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark temp)
  $ hg up temp
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark temp)
  $ hg bookmark --delete temp

Log is -f by default

  $ hg log -G -T '{desc}\n'
  @  b
  │
  o  a
  
  $ hg log -G -T '{desc}\n' --all
  o  a2
  │
  │ @  b
  ├─╯
  o  a
  
Dirty update to different rev fails with --check

  $ echo x >> a
  $ hg st
  M a
  $ hg goto ".^" --check
  abort: uncommitted changes
  [255]

Dirty update allowed to same rev, with no conflicts, and --clean

  $ hg goto .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg goto ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  hint[update-prev]: use 'hg prev' to move to the parent changeset
  hint[hint-ack]: use 'hg hint --ack update-prev' to silence these hints
  $ hg goto --clean 'desc(b)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Log on dir's works

  $ hg log -T '{desc}\n' dir
  b

  $ hg log -T '{desc}\n' -I 'dir/*'
  b

Empty rebase fails

  $ hg rebase
  abort: you must specify a destination (-d) for the rebase
  [255]
  $ hg rebase -d 'desc(a2)'
  rebasing * "b" (glob)

Empty rebase returns exit code 0:

  $ hg rebase -s tip -d "tip^1"
  nothing to rebase

Rebase fast forwards bookmark

  $ hg book -r 'desc(a2)' mybook
  $ hg up -q mybook
  $ hg log -G -T '{desc} {bookmarks}\n'
  @  a2 mybook
  │
  o  a
  
  $ hg rebase -d 'desc(b)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{desc} {bookmarks}\n'
  @  b mybook
  │
  o  a2
  │
  o  a
  
Rebase works with hyphens

  $ hg book -r 'desc(a2)' hyphen-book
  $ hg book -r 'desc(b)' hyphen-dest
  $ hg up -q hyphen-book
  $ hg log --all -G -T '{desc} {bookmarks}\n'
  o  b hyphen-dest mybook
  │
  @  a2 hyphen-book
  │
  o  a
  
  $ hg rebase -d hyphen-dest
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --all -G -T '{desc} {bookmarks}\n'
  @  b hyphen-book hyphen-dest mybook
  │
  o  a2
  │
  o  a
  
Rebase is blocked if you have conflicting changes

  $ hg up -q 3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ echo y > a
  $ hg rebase -d tip
  abort: 1 conflicting file changes:
   a
  (commit, shelve, goto --clean to discard all your changes, or update --merge to merge them)
  [255]
  $ hg revert -q --all
  $ hg up -qC hyphen-book
  $ rm a.orig

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
#if no-osx
  $ hg grep -V ''
  [123]
#endif

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
#if osx
  $ hg grep 'str([0-9])'
  [1]
#else
  $ hg grep 'str([0-9])'
  [123]
#endif
  $ hg grep 'str\([0-9]\)'
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub
#if osx
  $ hg grep -F 'str[0-9]'
  [1]
#else
  $ hg grep -F 'str[0-9]'
  [123]
#endif
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
  $ hg grep str1 'glob:*v'
  -v:str1-v
  $ hg grep str1 'file with space'
  file with space:str1space
  $ hg grep str1 'glob:*with*'
  file with space:str1space
  $ hg grep str1 'glob:*f1'
  f1:str1f1
  $ hg grep str1 subdir1
  subdir1/subf1:str1sub
  $ hg grep str1 'glob:**/*f1'
  f1:str1f1
  subdir1/subf1:str1sub

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

Test that --root-relative and patterns abort
  $ hg status --root-relative ""
  abort: --root-relative not supported with patterns
  (run from the repo root instead)
  [255]

Don't break automation
  $ HGPLAIN=1 hg status
  A dir1/-v
  A dir1/f1
  A dir1/file with space
  A dir1/subdir1/subf1

This tag is kept to keep the rest of the test consistent:
  $ echo >> ../.hgtags
  $ hg commit -Aqm "add foo tag"

Test graft date when tweakdefaults.graftkeepdate is not set
  $ hg revert -a -q
  $ hg up -q 'desc(a2)'
  $ hg graft -q 'desc(b) & mybook'
  $ hg log -T "{desc}\n" -d "yesterday to today"
  b

Test graft date when tweakdefaults.graftkeepdate is not set and --date is provided
  $ hg up -q 'desc(a2)'
  $ hg graft -q 'desc(b) & mybook' --date "1 1"
  $ hg log -l 1 -T "{date} {desc}\n"
  1.01 b

Test graft date when tweakdefaults.graftkeepdate is set
  $ hg up -q 'desc(a2)'
  $ hg graft -q 'max(desc(b))' --config tweakdefaults.graftkeepdate=True
  $ hg log -l 1 -T "{date} {desc}\n"
  1.01 b

Test amend date when tweakdefaults.amendkeepdate is not set
  $ hg up -q 'desc(a2)'
  $ echo x > a
  $ hg commit -Aqm "commit for amend"
  $ echo x > a
  $ hg amend -q -m "amended message"
  $ hg log -T "{desc}\n" -d "yesterday to today"
  amended message

Test amend date when tweakdefaults.amendkeepdate is set
  $ touch new_file
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ hg amend -q -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ hg log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test amend --to doesn't give a flag error when tweakdefaults.amendkeepdate is set
  $ echo q > new_file
  $ hg log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test commit --amend date when tweakdefaults.amendkeepdate is set
  $ echo a >> new_file
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ hg commit -q --amend -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ hg log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test commit --amend date when tweakdefaults.amendkeepdate is not set and --date is provided
  $ echo xxx > a
  $ hg commit -d "0 0" -Aqm "commit for amend"
  $ echo x > a
  $ hg commit -q --amend -m "amended message" --date "1 1"
  $ hg log -l 1 -T "{date} {desc}\n"
  1.01 amended message

Test rebase date when tweakdefaults.rebasekeepdate is not set
  $ echo test_1 > rebase_dest
  $ hg commit --date "1 1" -Aqm "dest commit for rebase"
  $ hg bookmark rebase_dest_test_1
  $ hg up -q ".^"
  hint[update-prev]: use 'hg prev' to move to the parent changeset
  hint[hint-ack]: use 'hg hint --ack update-prev' to silence these hints
  $ echo test_1 > rebase_source
  $ hg commit --date "1 1" -Aqm "source commit for rebase"
  $ hg bookmark rebase_source_test_1
  $ hg rebase -q -s rebase_source_test_1 -d rebase_dest_test_1
  $ hg log -l 1 -T "{desc}\n" -d "yesterday to today"
  source commit for rebase

Test rebase date when tweakdefaults.rebasekeepdate is set
  $ echo test_2 > rebase_dest
  $ hg commit -Aqm "dest commit for rebase"
  $ hg bookmark rebase_dest_test_2
  $ hg up -q ".^"
  hint[update-prev]: use 'hg prev' to move to the parent changeset
  hint[hint-ack]: use 'hg hint --ack update-prev' to silence these hints
  $ echo test_2 > rebase_source
  $ hg commit -Aqm "source commit for rebase"
  $ hg bookmark rebase_source_test_2
  $ hg rebase -q -s rebase_source_test_2 -d rebase_dest_test_2 --config tweakdefaults.rebasekeepdate=True
  $ hg log -l 2 -T "{date} {desc}\n"
  0.00 source commit for rebase
  0.00 dest commit for rebase

Test histedit date when tweakdefaults.histeditkeepdate is set
  $ hg bookmark histedit_test
  $ echo test_1 > histedit_1
  $ hg commit -Aqm "commit 1 for histedit"
  $ echo test_2 > histedit_2
  $ hg commit -Aqm "commit 2 for histedit"
  $ echo test_3 > histedit_3
  $ hg commit -Aqm "commit 3 for histedit"
  $ hg histedit "desc('commit 1 for histedit')" --commands - --config tweakdefaults.histeditkeepdate=True 2>&1 <<EOF| fixbundle
  > pick 22
  > pick 24
  > pick 23
  > EOF
  $ hg log -l 3 -T "{date} {desc}\n"
  0.00 commit 2 for histedit
  0.00 commit 3 for histedit
  0.00 commit 1 for histedit

Test histedit date when tweakdefaults.histeditkeepdate is not set
  $ hg histedit "desc('commit 1 for histedit')" --commands - 2>&1 <<EOF| fixbundle
  > pick 22
  > pick 26
  > pick 25
  > EOF
  $ hg log -l 2 -T "{desc}\n" -d "yesterday to today"
  commit 3 for histedit
  commit 2 for histedit

Test diff --per-file-stat
  $ echo a >> a
  $ echo b > b
  $ hg add b
  $ hg ci -m A
  $ hg diff -r ".^" -r . --per-file-stat-json
  {"dir1/foo/a": {"adds": 1, "isbinary": false, "removes": 0}, "dir1/foo/b": {"adds": 1, "isbinary": false, "removes": 0}}

Test rebase with showupdated=True
  $ cd $TESTTMP
  $ hg init showupdated
  $ cd showupdated
  $ cat >> .hg/hgrc <<EOF
  > [tweakdefaults]
  > showupdated=True
  > rebasekeepdate=True
  > EOF
  $ touch a && hg commit -Aqm a
  $ touch b && hg commit -Aqm b
  $ hg up -q 'desc(a)'
  $ touch c && hg commit -Aqm c
  $ hg log -G -T '{node} {bookmarks}' -r 'all()'
  @  d5e255ef74f8ec83b3a2a3f3254c699451c89e29
  │
  │ o  0e067c57feba1a5694ca4844f05588bb1bf82342
  ├─╯
  o  3903775176ed42b1458a6281db4a0ccf4d9f287a
  
  $ hg rebase -r 'desc(b)' -d 'desc(c)'
  rebasing 0e067c57feba "b"
  0e067c57feba -> a602e0d56f83 "b"

Test rebase with showupdate=True and a lot of source revisions

  $ newrepo
  $ setconfig tweakdefaults.showupdated=1 tweakdefaults.rebasekeepdate=1
  $ drawdag << 'EOS'
  > B C
  > |/
  > | D
  > |/
  > | E
  > |/
  > | F
  > |/
  > | G
  > |/
  > | H
  > |/
  > | I
  > |/
  > | J
  > |/
  > | K
  > |/
  > | L
  > |/
  > | M
  > |/
  > | N
  > |/
  > | O
  > |/
  > | P
  > |/
  > | Q
  > |/
  > A Z
  > EOS
  $ hg up -q 'desc(A)'
  $ hg rebase -r 'all() - roots(all())' -d 'desc(Z)'
  rebasing 112478962961 "B"
  rebasing dc0947a82db8 "C"
  rebasing b18e25de2cf5 "D"
  rebasing 7fb047a69f22 "E"
  rebasing 8908a377a434 "F"
  rebasing 6fa3874a3b67 "G"
  rebasing 575c4b5ec114 "H"
  rebasing 08ebfeb61bac "I"
  rebasing a0a5005cec67 "J"
  rebasing 83780307a7e8 "K"
  rebasing e131637a1cb6 "L"
  rebasing 699bc4b6fa22 "M"
  rebasing d19785b612fc "N"
  rebasing f8b24e0bba16 "O"
  rebasing febec53a8012 "P"
  rebasing b768a41fb64f "Q"
  112478962961 -> d1a90b33c3e4 "B"
  dc0947a82db8 -> 748dc89fb512 "C"
  b18e25de2cf5 -> bb5b4c942ce7 "D"
  7fb047a69f22 -> 84c88622d1aa "E"
  8908a377a434 -> ac569f2619af "F"
  6fa3874a3b67 -> 1f222ffda182 "G"
  575c4b5ec114 -> 662a28166552 "H"
  08ebfeb61bac -> 677e16fc90a1 "I"
  a0a5005cec67 -> 47e966978ada "J"
  83780307a7e8 -> 3ad2160089ee "K"
  ...
  b768a41fb64f -> 49a4c1a656cc "Q"

Test rebase with showupdate=True and a long commit message

  $ hg up -q 'desc(A)'
  $ echo 1 > longfile
  $ hg commit -qm "This is a long commit message which will be truncated." -A longfile
  $ hg rebase -r . -d 'desc(Z)'
  rebasing f5bef8190a99 "This is a long commit message which will be truncated."
  f5bef8190a99 -> 8df4b79a5414 "This is a long commit message which will be tru..."
