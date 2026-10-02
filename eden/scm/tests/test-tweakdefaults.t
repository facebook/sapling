#testcases ruststatus pythonstatus

#if pythonstatus
  $ setconfig status.use-rust=false
#endif

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable amend histedit rebase
  $ setconfig commands.update.check=noconflict
  $ setconfig ui.suggesthgprev=True

These are the actual default values for tweakdefaults.
  $ setconfig tweakdefaults.logdefaultfollow=true tweakdefaults.graftkeepdate=false

Setup repo

  $ configure modern
  $ newclientrepo repo
  $ touch a
  $ sl commit -Aqm a
  $ mkdir dir
  $ touch dir/b
  $ sl commit -Aqm b
  $ sl up -q 'desc(a)'
  $ echo x >> a
  $ sl commit -Aqm a2
  $ sl up -q 'desc(b)'

Updating to a specific date isn't blocked by our extensions'

  $ sl bookmark temp
  $ sl up -d "<today"
  hint[date-option]: --date performs a slow scan. Consider using `bsearch` revset (sl help revset) instead.
  found revision ae5108b653e2f2d15099970dec82ee0198e23d98 from Thu Jan 01 00:00:00 1970 +0000
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark temp)
  $ sl up temp
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark temp)
  $ sl bookmark --delete temp

Log is -f by default

  $ sl log -G -T '{desc}\n'
  @  b
  │
  o  a
  
  $ sl log -G -T '{desc}\n' --all
  o  a2
  │
  │ @  b
  ├─╯
  o  a
  
Dirty update to different rev fails with --check

  $ echo x >> a
  $ sl st
  M a
  $ sl goto ".^" --check
  abort: uncommitted changes
  [255]

Dirty update allowed to same rev, with no conflicts, and --clean

  $ sl goto .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl goto ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  hint[update-prev]: use 'sl prev' to move to the parent changeset
  hint[hint-ack]: use 'sl hint --ack update-prev' to silence these hints
  $ sl goto --clean 'desc(b)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Log on dir's works

  $ sl log -T '{desc}\n' dir
  b

  $ sl log -T '{desc}\n' -I 'dir/*'
  b

Empty rebase fails

  $ sl rebase
  abort: you must specify a destination (-d) for the rebase
  [255]
  $ sl rebase -d 'desc(a2)'
  rebasing * "b" (glob)

Empty rebase returns exit code 0:

  $ sl rebase -s tip -d "tip^1"
  nothing to rebase

Rebase fast forwards bookmark

  $ sl book -r 'desc(a2)' mybook
  $ sl up -q mybook
  $ sl log -G -T '{desc} {bookmarks}\n'
  @  a2 mybook
  │
  o  a
  
  $ sl rebase -d 'desc(b)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  nothing to rebase - fast-forwarded to 5e0171bd5ee2

  $ sl log --all -G -T '{desc} {bookmarks}\n'
  @  b mybook
  │
  o  a2
  │
  o  a
  
Rebase works with hyphens

  $ sl book -r 'desc(a2)' hyphen-book
  $ sl book -r 'desc(b)' hyphen-dest
  $ sl up -q hyphen-book
  $ sl log --all -G -T '{desc} {bookmarks}\n'
  o  b hyphen-dest mybook
  │
  @  a2 hyphen-book
  │
  o  a
  
  $ sl rebase -d hyphen-dest
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  nothing to rebase - fast-forwarded to 5e0171bd5ee2

  $ sl log -G -T '{desc} {bookmarks}\n'
  @  b hyphen-book hyphen-dest mybook
  │
  o  a2
  │
  o  a
  
Rebase is blocked if you have conflicting changes

  $ sl up -q 3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ echo y > a
  $ sl rebase -d tip
  abort: 1 conflicting file changes:
   a
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]
  $ sl revert -q --all
  $ sl up -qC hyphen-book
  $ rm a.orig

Grep options work

  $ mkdir -p dir1/subdir1
  $ echo str1f1 >> dir1/f1
  $ echo str1-v >> dir1/-v
  $ echo str1space >> 'dir1/file with space'
  $ echo str1sub >> dir1/subdir1/subf1
  $ sl add -q dir1

  $ sl grep x
  a:x
  $ sl grep -i X
  a:x
  $ sl grep -l x
  a
  $ sl grep -n x
  a:1:x
#if no-osx
  $ sl grep -V ''
  [1]
#endif

Make sure grep works in subdirectories and with strange filenames
  $ cd dir1
  $ sl grep str1 | sort
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub
  $ sl grep str1 'relre:f[0-9]+' | sort
  f1:str1f1
  subdir1/subf1:str1sub

Basic vs extended regular expressions
  $ sl grep 'str\([0-9]\)'
  [1]
  $ sl grep 'str([0-9])' | sort
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub
  $ sl grep -F 'str[0-9]'
  [1]
  $ sl grep -E 'str([0-9])' | sort
  -v:str1-v
  f1:str1f1
  file with space:str1space
  subdir1/subf1:str1sub

Crazy filenames
  $ sl grep str1 -- -v
  -v:str1-v
  $ sl grep str1 'glob:*v'
  -v:str1-v
  $ sl grep str1 'file with space'
  file with space:str1space
  $ sl grep str1 'glob:*with*'
  file with space:str1space
  $ sl grep str1 'glob:*f1'
  f1:str1f1
  $ sl grep str1 subdir1
  subdir1/subf1:str1sub
  $ sl grep str1 'glob:**/*f1' | sort
  f1:str1f1
  subdir1/subf1:str1sub

Test that status is default relative
  $ mkdir foo
  $ cd foo
  $ sl status
  A ../-v
  A ../f1
  A ../file with space
  A ../subdir1/subf1
  $ sl status --root-relative
  A dir1/-v
  A dir1/f1
  A dir1/file with space
  A dir1/subdir1/subf1
  $ sl status .
  $ sl status ../subdir1
  A ../subdir1/subf1

Don't break automation
  $ HGPLAIN=1 sl status
  A dir1/-v
  A dir1/f1
  A dir1/file with space
  A dir1/subdir1/subf1

But can still manually disable root-relative:
  $ HGPLAIN=1 sl status --no-root-relative
  A ../-v
  A ../f1
  A ../file with space
  A ../subdir1/subf1

This tag is kept to keep the rest of the test consistent:
  $ echo >> ../.hgtags
  $ sl commit -Aqm "add foo tag"

Test graft date when tweakdefaults.graftkeepdate is not set
  $ sl revert -a -q
  $ sl up -q 'desc(a2)'
  $ sl graft -q 'desc(b) & mybook' --config devel.default-date='2 2'
  $ sl log -l 1 -T "{date} {desc}\n"
  2.02 b

Test graft date when tweakdefaults.graftkeepdate is not set and --date is provided
  $ sl up -q 'desc(a2)'
  $ sl graft -q 'desc(b) & mybook' --date "1 1"
  $ sl log -l 1 -T "{date} {desc}\n"
  1.01 b

Test graft date when tweakdefaults.graftkeepdate is set
  $ sl up -q 'desc(a2)'
  $ sl graft -q 'max(desc(b))' --config tweakdefaults.graftkeepdate=True
  $ sl log -l 1 -T "{date} {desc}\n"
  1.01 b

Test amend date when tweakdefaults.amendkeepdate is not set
  $ sl up -q 'desc(a2)'
  $ echo x > a
  $ sl commit -Aqm "commit for amend"
  $ echo x > a
  $ sl amend -q -m "amended message" --config devel.default-date='2 2'
  $ sl log -l 1 -T "{date} {desc}\n"
  2.02 amended message

Test amend date when tweakdefaults.amendkeepdate is set
  $ touch new_file
  $ sl commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ sl amend -q -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ sl log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test amend --to doesn't give a flag error when tweakdefaults.amendkeepdate is set
  $ echo q > new_file
  $ sl log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test commit --amend date when tweakdefaults.amendkeepdate is set
  $ echo a >> new_file
  $ sl commit -d "0 0" -Aqm "commit for amend"
  $ echo x > new_file
  $ sl commit -q --amend -m "amended message" --config tweakdefaults.amendkeepdate=True
  $ sl log -l 1 -T "{date} {desc}\n"
  0.00 amended message

Test commit --amend date when tweakdefaults.amendkeepdate is not set and --date is provided
  $ echo xxx > a
  $ sl commit -d "0 0" -Aqm "commit for amend"
  $ echo x > a
  $ sl commit -q --amend -m "amended message" --date "1 1"
  $ sl log -l 1 -T "{date} {desc}\n"
  1.01 amended message

Test rebase date when tweakdefaults.rebasekeepdate is not set
  $ echo test_1 > rebase_dest
  $ sl commit --date "1 1" -Aqm "dest commit for rebase"
  $ sl bookmark rebase_dest_test_1
  $ sl up -q ".^"
  hint[update-prev]: use 'sl prev' to move to the parent changeset
  hint[hint-ack]: use 'sl hint --ack update-prev' to silence these hints
  $ echo test_1 > rebase_source
  $ sl commit --date "1 1" -Aqm "source commit for rebase"
  $ sl bookmark rebase_source_test_1
  $ sl rebase -q -s rebase_source_test_1 -d rebase_dest_test_1 --config devel.default-date='2 2'
  $ sl log -l 1 -T "{date} {desc}\n"
  2.02 source commit for rebase

Test rebase date when tweakdefaults.rebasekeepdate is set
  $ echo test_2 > rebase_dest
  $ sl commit -Aqm "dest commit for rebase"
  $ sl bookmark rebase_dest_test_2
  $ sl up -q ".^"
  hint[update-prev]: use 'sl prev' to move to the parent changeset
  hint[hint-ack]: use 'sl hint --ack update-prev' to silence these hints
  $ echo test_2 > rebase_source
  $ sl commit -Aqm "source commit for rebase"
  $ sl bookmark rebase_source_test_2
  $ sl rebase -q -s rebase_source_test_2 -d rebase_dest_test_2 --config tweakdefaults.rebasekeepdate=True
  $ sl log -l 2 -T "{date} {desc}\n"
  0.00 source commit for rebase
  0.00 dest commit for rebase

Test histedit date when tweakdefaults.histeditkeepdate is set
  $ sl bookmark histedit_test
  $ echo test_1 > histedit_1
  $ sl commit -Aqm "commit 1 for histedit"
  $ echo test_2 > histedit_2
  $ sl commit -Aqm "commit 2 for histedit"
  $ echo test_3 > histedit_3
  $ sl commit -Aqm "commit 3 for histedit"
  $ tglogp --limit 3
  @  1cd28f082aaa draft 'commit 3 for histedit' histedit_test
  │
  o  ae01c7e395dd draft 'commit 2 for histedit'
  │
  o  08116b82180a draft 'commit 1 for histedit'
  │
  ~
  $ sl histedit "desc('commit 1 for histedit')" --commands - --config tweakdefaults.histeditkeepdate=True 2>&1 <<EOF| fixbundle
  > pick 08116b82180a
  > pick 1cd28f082aaa
  > pick ae01c7e395dd
  > EOF
  $ sl log -l 3 -T "{date} {desc}\n"
  0.00 commit 2 for histedit
  0.00 commit 3 for histedit
  0.00 commit 1 for histedit

Test histedit date when tweakdefaults.histeditkeepdate is not set
  $ tglogp --limit 3
  @  8baf563aea27 draft 'commit 2 for histedit' histedit_test
  │
  o  0d8ef43b4a3b draft 'commit 3 for histedit'
  │
  o  08116b82180a draft 'commit 1 for histedit'
  │
  ~
  $ sl histedit "desc('commit 1 for histedit')" --config devel.default-date='2 2' --commands - 2>&1 <<EOF| fixbundle
  > pick 08116b82180a
  > pick 8baf563aea27
  > pick 0d8ef43b4a3b
  > EOF
  $ sl log -l 2 -T "{date} {desc}\n"
  2.02 commit 3 for histedit
  2.02 commit 2 for histedit

Test diff --per-file-stat
  $ echo a >> a
  $ echo b > b
  $ sl add b
  $ sl ci -m A
  $ sl diff -r ".^" -r . --per-file-stat-json
  {"dir1/foo/a": {"adds": 1, "isbinary": false, "removes": 0}, "dir1/foo/b": {"adds": 1, "isbinary": false, "removes": 0}}

Test rebase with showupdated=True
  $ cd $TESTTMP
  $ newclientrepo showupdated
  $ setconfig tweakdefaults.showupdated=True tweakdefaults.rebasekeepdate=True
  $ touch a && sl commit -Aqm a
  $ touch b && sl commit -Aqm b
  $ sl up -q 'desc(a)'
  $ touch c && sl commit -Aqm c
  $ sl log -G -T '{node} {bookmarks}' -r 'all()'
  @  d5e255ef74f8ec83b3a2a3f3254c699451c89e29
  │
  │ o  0e067c57feba1a5694ca4844f05588bb1bf82342
  ├─╯
  o  3903775176ed42b1458a6281db4a0ccf4d9f287a
  
  $ sl rebase -r 'desc(b)' -d 'desc(c)'
  rebasing 0e067c57feba "b"
  0e067c57feba -> a602e0d56f83 "b"

Test rebase with showupdate=True and a lot of source revisions

  $ newclientrepo
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
  $ sl up -q 'desc(A)'
  $ sl rebase -r 'all() - roots(all())' -d 'desc(Z)'
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
  ... (6 more)
  b768a41fb64f -> 49a4c1a656cc "Q"

Test rebase with showupdate=True and a long commit message

  $ sl up -q 'desc(A)'
  $ echo 1 > longfile
  $ sl commit -qm "This is a long commit message which will be truncated." -A longfile
  $ sl rebase -r . -d 'desc(Z)'
  rebasing f5bef8190a99 "This is a long commit message which will be truncated."
  f5bef8190a99 -> 8df4b79a5414 "This is a long commit message which will be tru..."

The updated commits are listed in stack order. Beyond the limit (10 by
default), the bottom and top of every stack are always shown, and runs of other
commits are summarised:

  $ newclientrepo
  $ setconfig tweakdefaults.showupdated=1
  $ drawdag << 'EOS'
  > B8
  > |
  > B7
  > |
  > B6
  > |
  > B5
  > |
  > B4
  > |
  > B3    C6
  > |     |
  > B2    C5
  > |     |
  > B1    C4
  > |     |
  > |     C3
  > |     |
  > |     C2
  > |     |
  > |     C1
  > |    /
  > A   Z
  > EOS
  $ sl rebase -q -s $C1 -d $A
  $ sl rebase -r "(desc(B1)::) + (desc(C1)::)" -d $Z | grep -v rebasing
  a178212c3433 -> ff960b5d1b00 "B1"
  7fa699741696 -> 4d9d1021f658 "B2"
  c61260c6c7b7 -> c607629739f1 "B3"
  af983229a36e -> 79d63aca3e72 "B4"
  d02fe5346ae8 -> 551bea22b9bd "B5"
  dd51f6997e4a -> 7385c9531a88 "B6"
  4147bcdaf572 -> 63dba79a5a10 "B7"
  39956745e016 -> 1c7d6cd1acb7 "B8"
  a500ca6c39da -> 82de329fd685 "C1"
  ... (4 more)
  4644a203e6c5 -> 7f3ae96847cc "C6"

An agent sees up to 100 by default, and the limit can be configured:

  $ CODING_AGENT_METADATA=id=test_agent sl rebase -r "(desc(B1)::) + (desc(C1)::)" -d $A | grep -v rebasing
  ff960b5d1b00 -> fde90be5c9b2 "B1"
  4d9d1021f658 -> 91ea4d8cdcaa "B2"
  c607629739f1 -> 527fcd18f366 "B3"
  79d63aca3e72 -> 800fd3016513 "B4"
  551bea22b9bd -> 515f9c2e5c2d "B5"
  7385c9531a88 -> 44a1417024e2 "B6"
  63dba79a5a10 -> 269e60c050a0 "B7"
  1c7d6cd1acb7 -> 99d6863838de "B8"
  82de329fd685 -> 7b8178b5e09c "C1"
  ec0e1922f5de -> 1ab2ed16fbbe "C2"
  e60c0262edf6 -> ed316a8bef7e "C3"
  3703c21efe12 -> d1adcde7c19d "C4"
  cf2fc28c3a4e -> 9ed18568d410 "C5"
  7f3ae96847cc -> b3b9a76f0c5d "C6"
  $ sl rebase -r "(desc(B1)::) + (desc(C1)::)" -d $Z --config tweakdefaults.showupdatedlimit=5 | grep -v rebasing
  fde90be5c9b2 -> e7be97062340 "B1"
  91ea4d8cdcaa -> e973d93f21d4 "B2"
  ... (5 more)
  99d6863838de -> 28ca03fd4e39 "B8"
  7b8178b5e09c -> 452a01acc439 "C1"
  ... (4 more)
  b3b9a76f0c5d -> baaac3e0143d "C6"

A single omitted commit is shown rather than summarised:

  $ sl rebase -r "desc(B1)::" -d $A --config tweakdefaults.showupdatedlimit=7 | grep -v rebasing
  e7be97062340 -> 362999f839b7 "B1"
  e973d93f21d4 -> 0d214380f289 "B2"
  bfcb687a614e -> f80f377359a0 "B3"
  476f989a440f -> 3693f2232a9b "B4"
  d2c9ee29e6dd -> 603cbf2222b8 "B5"
  341786b23885 -> 966b1762463f "B6"
  319cd1bf50de -> 5cc92c86555c "B7"
  28ca03fd4e39 -> fc9a6f5dcf50 "B8"
