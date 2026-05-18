#modern-config-incompatible

#require no-eden

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig clone.use-rust=1

#testcases rustcheckout pythoncheckout

#if rustcheckout
  $ setconfig workingcopy.rust-checkout=true
#else
  $ setconfig workingcopy.rust-checkout=false
#endif

  $ configure dummyssh

Prepare repo a:

  $ sl init a
  $ cd a
  $ echo a > a
  $ sl add a
  $ sl commit -m test
  $ echo first line > b
  $ sl add b

Create a non-inlined filelog:

  $ sl debugsh -c 'open("data1", "wb").write("".join("%s\n" % x for x in range(10000)).encode("utf-8"))'
  $ for j in 0 1 2 3 4 5 6 7 8 9; do
  >   cat data1 >> b
  >   sl commit -m test
  > done

Default operation:

  $ sl clone . ../b
  updating to tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

  $ cat a
  a
  $ sl verify
  warning: verify does not actually check anything in this repo

Invalid dest '' must abort:

  $ sl clone . ''
  abort: empty destination path is not valid
  [255]

No update, with debug option:

  $ sl clone -U . ../c
  $ cd ../c

  $ cat a 2>/dev/null || echo "a not present"
  a not present
  $ sl verify
  warning: verify does not actually check anything in this repo

Default destination:

  $ mkdir ../d
  $ cd ../d
  $ sl clone ../a
  destination directory: a
  updating to tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd a
  $ sl cat a
  a
  $ cd ../..

Check that we drop the 'file:' from the path before writing the .hgrc:

  $ sl clone file:a e
  updating to tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ grep 'file:' e/.sl/config
  [1]

Check that path aliases are expanded:

  $ sl clone -q -U --config 'paths.foobar=a#0' foobar f
  $ sl -R f config paths.default
  $TESTTMP/a#0


Clone to '.':

  $ mkdir h
  $ cd h
  $ sl clone ../a .
  updating to tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..


*** Tests for option -u ***

Adding some more history to repo a:

  $ cd a
  $ echo the quick brown fox >a
  $ sl ci -m "hacked default"
  $ sl up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl bookmark stable
  $ echo some text >a
  $ sl ci -m "starting branch stable"
  $ echo some more text >a
  $ sl ci -m "another change for branch stable"
  $ sl up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark stable)
  $ sl parents
  commit:      7bc8ee83a26f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable
  

Repo a has two heads:

  $ sl heads
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

  $ sl clone --noupdate --updaterev 1 a ua
  abort: cannot specify both --noupdate and --updaterev
  [255]


Testing clone -u:

  $ sl clone -u . a ua
  updating to 7bc8ee83a26fd5fa6374a25e8f8248ea074e16a3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ sl -R ua heads
  commit:      7bc8ee83a26f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable

Same revision checked out in repo a and ua:

  $ sl -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ sl -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Testing clone -u <branch>:

  $ sl clone -u stable a ua
  updating to 4f44d5743f52b70e278b04871eab353996595b1d
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ sl -R ua heads
  commit:      4f44d5743f52
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable

Branch 'stable' is checked out:

  $ sl -R ua parents
  commit:      4f44d5743f52
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable

  $ rm -r ua


Testing default checkout:

  $ sl clone a ua
  updating to tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ sl -R ua heads
  commit:      4f44d5743f52
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable

  $ rm -r ua


Testing #<bookmark> (no longer works):

  $ sl clone -u . a#stable ua
  updating to 7bc8ee83a26fd5fa6374a25e8f8248ea074e16a3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ sl -R ua heads
  commit:      7bc8ee83a26f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable

Same revision checked out in repo a and ua:

  $ sl -R a parents --template "{node|short}\n"
  7bc8ee83a26f
  $ sl -R ua parents --template "{node|short}\n"
  7bc8ee83a26f

  $ rm -r ua


Test clone with special '@' bookmark:
  $ cd a
  $ sl bookmark -r a7949464abda @  # branch point of stable from default
  $ sl clone . ../i
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl id -i ../i
  a7949464abda
  $ rm -r ../i

  $ sl bookmark -f -r stable @
  $ sl bookmarks
     @                         4f44d5743f52
     stable                    4f44d5743f52
  $ sl clone . ../i
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl id -i ../i
  4f44d5743f52
  $ cd "$TESTTMP"


Testing failures:

  $ mkdir fail
  $ cd fail

No local source

  $ sl clone a b
  abort: repository a not found!
  [255]

  $ rm -rf b # work around bug with http clone


#if unix-permissions no-root

Inaccessible source

  $ mkdir a
  $ chmod 000 a
  $ sl clone a b
  abort: Permission denied
  [255]

Inaccessible destination

  $ sl init b
  $ cd b
  $ sl clone . ../a
  abort: Permission denied: ../a
  ...
  $ cd ..
  $ chmod 700 a
  $ rm -r a b

#endif


#if mkfifo fifo

Source of wrong type

  $ mkfifo a
  $ sl clone a b
  abort: repository a not found!
  [255]
  $ rm a

#endif

Default destination, same directory

  $ sl init q
  $ sl clone q
  destination directory: q
  abort: destination 'q' is not empty
  [255]

destination directory not empty

  $ mkdir a
  $ echo stuff > a/a
  $ sl clone q a
  abort: destination 'a' is not empty
  [255]


  $ cd ..

Test clone from the repository in (emulated) revlog format 0 (issue4203):

  $ mkdir issue4203
  $ mkdir -p src/.sl
  $ touch src/.sl/requires
  $ echo foo > src/foo
  $ sl -R src add src/foo
  abort: legacy dirstate implementations are no longer supported (path=$TESTTMP/src/.sl, requirements=set())!
  [255]
  $ sl -R src commit -m '#0'
  abort: legacy dirstate implementations are no longer supported (path=$TESTTMP/src/.sl, requirements=set())!
  [255]
  $ sl -R src log -q
  abort: legacy dirstate implementations are no longer supported (path=$TESTTMP/src/.sl, requirements=set())!
  [255]
  $ sl clone -U -q src dst
  abort: legacy dirstate implementations are no longer supported (path=$TESTTMP/src/.sl, requirements=set())!
  [255]
  $ sl -R dst log -q
  abort: repository dst not found!
  [255]

Create repositories to test auto sharing functionality

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share=
  > EOF

  $ sl init empty
  $ sl init source1a
  $ cd source1a
  $ echo initial1 > foo
  $ sl -q commit -A -m initial
  $ echo second > foo
  $ sl commit -m second
  $ cd ..

  $ sl init filteredrev0
  $ cd filteredrev0
  $ cat >> .sl/config << EOF
  > [experimental]
  > evolution.createmarkers=True
  > EOF
  $ echo initial1 > foo
  $ sl -q commit -A -m initial0
  $ sl -q up -r null
  $ echo initial2 > foo
  $ sl -q commit -A -m initial1
  $ sl debugobsolete c05d5c47a5cf81401869999f3d05f7d699d2b29a e082c1832e09a7d1e78b7fd49a592d372de854c8
  $ cd ..

  $ sl -q clone source1a source1b
  $ cd source1a
  $ sl bookmark bookA
  $ echo 1a > foo
  $ sl commit -m 1a
  $ cd ../source1b
  $ sl -q up -r 'desc(initial)'
  $ echo head1 > foo
  $ sl commit -m head1
  $ sl bookmark head1
  $ sl -q up -r 'desc(initial)'
  $ echo head2 > foo
  $ sl commit -m head2
  $ sl bookmark head2
  $ sl -q up -r 'desc(initial)'
  $ sl bookmark branch1
  $ echo branch1 > foo
  $ sl commit -m branch1
  $ sl -q up -r 'desc(initial)'
  $ sl bookmark branch2
  $ echo branch2 > foo
  $ sl commit -m branch2
  $ cd ..
  $ sl init source2
  $ cd source2
  $ echo initial2 > foo
  $ sl -q commit -A -m initial2
  $ echo second > foo
  $ sl commit -m second
  $ cd ..

Cloning without fsmonitor enabled does not print a warning for small repos

  $ sl clone a fsmonitor-default
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Lower the warning threshold to simulate a large repo

  $ cat >> $HGRCPATH << EOF
  > [fsmonitor]
  > warn_update_file_count = 2
  > EOF

We should see a warning about no fsmonitor on supported platforms
  $ setconfig checkout.use-rust=false

#if linuxormacos no-fsmonitor
  $ sl clone a nofsmonitor
  updating to bookmark @
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "sl help -e fsmonitor")
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#else
  $ sl clone a nofsmonitor
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We should not see warning about fsmonitor when it is enabled

#if fsmonitor
  $ sl clone a fsmonitor-enabled
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We can disable the fsmonitor warning

  $ sl --config fsmonitor.warn_when_unused=false clone a fsmonitor-disable-warning
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Loaded fsmonitor but disabled in config should still print warning

#if linuxormacos fsmonitor
  $ sl --config fsmonitor.mode=off clone a fsmonitor-mode-off
  updating to bookmark @
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "sl help -e fsmonitor") (fsmonitor !)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

Warning not printed if working directory isn't empty

  $ sl -q clone a fsmonitor-update
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "sl help -e fsmonitor") (?)
  $ cd fsmonitor-update
  $ sl up acb14030fe0a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl up cf0fe1914066
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

`sl update` from null revision also prints

  $ sl up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved

#if linuxormacos no-fsmonitor
  $ sl up cf0fe1914066
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "sl help -e fsmonitor") (?)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#else
  $ sl up cf0fe1914066
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

  $ cd ..
