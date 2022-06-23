#chg-compatible
  $ setconfig experimental.allowfilepeer=True
  $ setconfig clone.use-rust=1

  $ disable treemanifest
  $ configure dummyssh

Prepare repo a:

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add a
  $ hg commit -m test
  $ echo first line > b
  $ hg add b

Create a non-inlined filelog:

  $ hg debugsh -c 'open("data1", "wb").write("".join("%s\n" % x for x in range(10000)).encode("utf-8"))'
  $ for j in 0 1 2 3 4 5 6 7 8 9; do
  >   cat data1 >> b
  >   hg commit -m test
  > done

Default operation:

  $ hg clone . ../b
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

  $ cat a
  a
  $ hg verify
  warning: verify does not actually check anything in this repo

Invalid dest '' must abort:

  $ hg clone . ''
  abort: empty destination path is not valid
  [255]

No update, with debug option:

#if hardlink
  $ hg --debug clone -U . ../c --config progress.debug=true
  progress: linking: 1
  progress: linking: 2
  progress: linking: 3
  progress: linking: 4
  progress: linking: 5
  progress: linking: 6
  progress: linking: 7
  progress: linking: 8
  progress: linking: 9
  progress: linking: 10
  progress: linking: 11
  progress: linking: 12
  progress: linking: 13
  progress: linking: 14
  progress: linking: 15
  progress: linking: 16
  progress: linking: 17
  progress: linking: 18
  progress: linking: 19
  progress: linking: 20
  progress: linking: 21
  progress: linking: 22
  progress: linking: 23
  progress: linking (end)
  copied 23 files
#else
  $ hg --debug clone -U . ../c --config progress.debug=true
  linking: 1
  copying: 2
  copying: 3
  copying: 4
  copying: 5
  copying: 6
  copying: 7
  copying: 8
  copied 8 files
#endif
  $ cd ../c

  $ cat a 2>/dev/null || echo "a not present"
  a not present
  $ hg verify
  warning: verify does not actually check anything in this repo

Default destination:

  $ mkdir ../d
  $ cd ../d
  $ hg clone ../a
  destination directory: a
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ hg cat a
  a
  $ cd ../..

Check that we drop the 'file:' from the path before writing the .hgrc:

  $ hg clone file:a e
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ grep 'file:' e/.hg/hgrc
  [1]

Check that path aliases are expanded:

  $ hg clone -q -U --config 'paths.foobar=a#0' foobar f
  $ hg -R f showconfig paths.default
  $TESTTMP/a#0

Use --pull:

  $ hg clone --pull a g
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R g verify
  warning: verify does not actually check anything in this repo

Invalid dest '' with --pull must abort (issue2528):

  $ hg clone --pull a ''
  abort: empty destination path is not valid
  [255]

Clone to '.':

  $ mkdir h
  $ cd h
  $ hg clone ../a .
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..


*** Tests for option -u ***

Adding some more history to repo a:

  $ cd a
  $ echo the quick brown fox >a
  $ hg ci -m "hacked default"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmark stable
  $ echo some text >a
  $ hg ci -m "starting branch stable"
  $ echo some more text >a
  $ hg ci -m "another change for branch stable"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark stable)
  $ hg parents
  commit:      7bc8ee83a26f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable
  

Repo a has two heads:

  $ hg heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

  $ cd ..


Testing --noupdate with --updaterev (must abort):

  $ hg clone --noupdate --updaterev 1 a ua
  abort: cannot specify both --noupdate and --updaterev
  [255]


Testing clone -u:

  $ hg clone -u . a ua
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ hg -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Testing clone --pull -u:

  $ hg clone --pull -u . a ua
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ hg -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Testing clone -u <branch>:

  $ hg clone -u stable a ua
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Branch 'stable' is checked out:

  $ hg -R ua parents
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

  $ rm -r ua


Testing default checkout:

  $ hg clone a ua
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

  $ rm -r ua


Testing #<bookmark> (no longer works):

  $ hg clone -u . a#stable ua
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  commit:      3aa88e8a4d5f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ hg -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Testing -u -r <branch>:

  $ hg clone -u . -r stable a ua
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ hg -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Testing -r <branch>:

  $ hg clone -r stable a ua
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

Branch 'stable' is checked out:

  $ hg -R ua parents
  commit:      4f44d5743f52
  bookmark:    stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

  $ rm -r ua


Test clone with special '@' bookmark:
  $ cd a
  $ hg bookmark -r a7949464abda @  # branch point of stable from default
  $ hg clone . ../i
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -i ../i
  a7949464abda
  $ rm -r ../i

  $ hg bookmark -f -r stable @
  $ hg bookmarks
     @                         4f44d5743f52
     stable                    4f44d5743f52
  $ hg clone . ../i
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -i ../i
  4f44d5743f52
  $ cd "$TESTTMP"


Testing failures:

  $ mkdir fail
  $ cd fail

No local source

  $ hg clone a b
  abort: repository a not found!
  [255]

  $ rm -rf b # work around bug with http clone


#if unix-permissions no-root

Inaccessible source

  $ mkdir a
  $ chmod 000 a
  $ hg clone a b
  abort: repository a not found!
  [255]

Inaccessible destination

  $ hg init b
  $ cd b
  $ hg clone . ../a
  abort: Permission denied: ../a
  (current process runs with uid 42) (?)
  (../a: mode 0o52, uid 42, gid 42) (?)
  (..: mode 0o52, uid 42, gid 42) (?)
  [255]
  $ cd ..
  $ chmod 700 a
  $ rm -r a b

#endif


#if mkfifo fifo

Source of wrong type

  $ mkfifo a
  $ hg clone a b
  abort: repository a not found!
  [255]
  $ rm a

#endif

Default destination, same directory

  $ hg init q
  $ hg clone q
  destination directory: q
  abort: destination 'q' is not empty
  [255]

destination directory not empty

  $ mkdir a
  $ echo stuff > a/a
  $ hg clone q a
  abort: destination 'a' is not empty
  [255]


#if unix-permissions no-root

leave existing directory in place after clone failure

  $ hg init c
  $ cd c
  $ echo c > c
  $ hg commit -A -m test
  adding c
  $ chmod -rx .hg/store/data
  $ cd ..
  $ mkdir d
  $ hg clone c d 2> err
  [255]
  $ test -d d
  $ test -d d/.hg
  [1]

re-enable perm to allow deletion

  $ chmod +rx c/.hg/store/data

#endif

  $ cd ..

Test clone from the repository in (emulated) revlog format 0 (issue4203):

  $ mkdir issue4203
  $ mkdir -p src/.hg
  $ echo foo > src/foo
  $ hg -R src add src/foo
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ hg -R src commit -m '#0'
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ hg -R src log -q
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ hg clone -U -q src dst
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ hg -R dst log -q
  abort: repository dst not found!
  [255]

Create repositories to test auto sharing functionality

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share=
  > EOF

  $ hg init empty
  $ hg init source1a
  $ cd source1a
  $ echo initial1 > foo
  $ hg -q commit -A -m initial
  $ echo second > foo
  $ hg commit -m second
  $ cd ..

  $ hg init filteredrev0
  $ cd filteredrev0
  $ cat >> .hg/hgrc << EOF
  > [experimental]
  > evolution.createmarkers=True
  > EOF
  $ echo initial1 > foo
  $ hg -q commit -A -m initial0
  $ hg -q up -r null
  $ echo initial2 > foo
  $ hg -q commit -A -m initial1
  $ hg debugobsolete c05d5c47a5cf81401869999f3d05f7d699d2b29a e082c1832e09a7d1e78b7fd49a592d372de854c8
  $ cd ..

  $ hg -q clone --pull source1a source1b
  $ cd source1a
  $ hg bookmark bookA
  $ echo 1a > foo
  $ hg commit -m 1a
  $ cd ../source1b
  $ hg -q up -r 'desc(initial)'
  $ echo head1 > foo
  $ hg commit -m head1
  $ hg bookmark head1
  $ hg -q up -r 'desc(initial)'
  $ echo head2 > foo
  $ hg commit -m head2
  $ hg bookmark head2
  $ hg -q up -r 'desc(initial)'
  $ hg bookmark branch1
  $ echo branch1 > foo
  $ hg commit -m branch1
  $ hg -q up -r 'desc(initial)'
  $ hg bookmark branch2
  $ echo branch2 > foo
  $ hg commit -m branch2
  $ cd ..
  $ hg init source2
  $ cd source2
  $ echo initial2 > foo
  $ hg -q commit -A -m initial2
  $ echo second > foo
  $ hg commit -m second
  $ cd ..

Cloning without fsmonitor enabled does not print a warning for small repos

  $ hg clone a fsmonitor-default
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Lower the warning threshold to simulate a large repo

  $ cat >> $HGRCPATH << EOF
  > [fsmonitor]
  > warn_update_file_count = 2
  > EOF

We should see a warning about no fsmonitor on supported platforms

#if linuxormacos no-fsmonitor
  $ hg clone a nofsmonitor
  updating to bookmark @
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor")
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#else
  $ hg clone a nofsmonitor
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We should not see warning about fsmonitor when it is enabled

#if fsmonitor
  $ hg clone a fsmonitor-enabled
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We can disable the fsmonitor warning

  $ hg --config fsmonitor.warn_when_unused=false clone a fsmonitor-disable-warning
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Loaded fsmonitor but disabled in config should still print warning

#if linuxormacos fsmonitor
  $ hg --config fsmonitor.mode=off clone a fsmonitor-mode-off
  updating to bookmark @
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor") (fsmonitor !)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

Warning not printed if working directory isn't empty

  $ hg -q clone a fsmonitor-update
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor") (?)
  $ cd fsmonitor-update
  $ hg up acb14030fe0a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark @)
  $ hg up cf0fe1914066
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

`hg update` from null revision also prints

  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

#if linuxormacos no-fsmonitor
  $ hg up cf0fe1914066
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor")
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#else
  $ hg up cf0fe1914066
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

  $ cd ..

