  $ setconfig extensions.treemanifest=!
  $ . helpers-usechg.sh

Prepare repo a:

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add a
  $ hg commit -m test
  $ echo first line > b
  $ hg add b

Create a non-inlined filelog:

  $ $PYTHON -c 'file("data1", "wb").write("".join("%s\n" % x for x in range(10000)))'
  $ for j in 0 1 2 3 4 5 6 7 8 9; do
  >   cat data1 >> b
  >   hg commit -m test
  > done

List files in store/data (should show a 'b.d'):

  $ for i in .hg/store/data/*; do
  >   echo $i
  > done
  .hg/store/data/a.i
  .hg/store/data/b.d
  .hg/store/data/b.i

Default operation:

  $ hg clone . ../b
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

  $ cat a
  a
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 11 changesets, 11 total revisions

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
  progress: linking (end)
  linked 9 files
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
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 11 changesets, 11 total revisions

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
  added 11 changesets with 11 changes to 2 files
  new changesets acb14030fe0a:a7949464abda
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R g verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 11 changesets, 11 total revisions

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
  $ hg tag ref1
  $ echo the quick brown fox >a
  $ hg ci -m "hacked default"
  $ hg up ref1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmark stable
  $ echo some text >a
  $ hg ci -m "starting branch stable"
  $ hg tag ref2
  $ echo some more text >a
  $ hg ci -m "another change for branch stable"
  $ hg up ref2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark stable)
  $ hg parents
  changeset:   13:7bc8ee83a26f
  tag:         ref2
  parent:      10:a7949464abda
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable
  

Repo a has two heads:

  $ hg heads
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
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
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
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
  added 16 changesets with 16 changes to 3 files
  new changesets acb14030fe0a:7b0a8591eda2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
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
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Branch 'stable' is checked out:

  $ hg -R ua parents
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

  $ rm -r ua


Testing default checkout:

  $ hg clone a ua
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
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
  changeset:   15:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
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
  added 14 changesets with 14 changes to 3 files
  new changesets acb14030fe0a:7b0a8591eda2
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  changeset:   13:7b0a8591eda2
  bookmark:    stable
  tag:         tip
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
  added 14 changesets with 14 changes to 3 files
  new changesets acb14030fe0a:7b0a8591eda2
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  changeset:   13:7b0a8591eda2
  bookmark:    stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

Branch 'stable' is checked out:

  $ hg -R ua parents
  changeset:   13:7b0a8591eda2
  bookmark:    stable
  tag:         tip
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
     @                         15:7b0a8591eda2
     stable                    15:7b0a8591eda2
  $ hg clone . ../i
  updating to bookmark @
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -i ../i
  7b0a8591eda2
  $ cd "$TESTTMP"


Testing failures:

  $ mkdir fail
  $ cd fail

No local source

  $ hg clone a b
  abort: repository a not found!
  [255]

No remote source

#if windows
  $ hg clone http://$LOCALIP:3121/a b
  abort: error: * (glob)
  [255]
#else
  $ hg clone http://$LOCALIP:3121/a b
  abort: error: *refused* (glob)
  [255]
#endif
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
  abort: Permission denied: '../a'
  [255]
  $ cd ..
  $ chmod 700 a
  $ rm -r a b

#endif


#if fifo

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
  abort: repo is corrupted: 00changelog.i
  [255]
  $ hg -R src commit -m '#0'
  abort: repo is corrupted: 00changelog.i
  [255]
  $ hg -R src log -q
  abort: repo is corrupted: 00changelog.i
  [255]
  $ hg clone -U -q src dst
  abort: repo is corrupted: 00changelog.i
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
  obsoleted 1 changesets
  $ cd ..

  $ hg -q clone --pull source1a source1b
  $ cd source1a
  $ hg bookmark bookA
  $ echo 1a > foo
  $ hg commit -m 1a
  $ cd ../source1b
  $ hg -q up -r 0
  $ echo head1 > foo
  $ hg commit -m head1
  $ hg bookmark head1
  $ hg -q up -r 0
  $ echo head2 > foo
  $ hg commit -m head2
  $ hg bookmark head2
  $ hg -q up -r 0
  $ hg bookmark branch1
  $ echo branch1 > foo
  $ hg commit -m branch1
  $ hg -q up -r 0
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

Clone with auto share from an empty repo should not result in share

  $ mkdir share
  $ hg --config share.pool=share clone empty share-empty
  (not using pooled storage: remote appears to be empty)
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls share
  $ test -d share-empty/.hg/store
  $ test -f share-empty/.hg/sharedpath
  [1]

Clone with auto share from a repo with filtered revision 0 should not result in share

  $ hg --config share.pool=share clone filteredrev0 share-filtered
  (not using pooled storage: unable to resolve identity of remote)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets e082c1832e09
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Clone from repo with content should result in shared store being created

  $ hg --config share.pool=share clone source1a share-dest1a
  (sharing from new pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  new changesets b5f04eac9d8f:e5bfe23c0b47
  searching for changes
  no changes found
  adding remote bookmark bookA
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

The shared repo should have been created

  $ ls share
  b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1

The destination should point to it

  $ cat share-dest1a/.hg/sharedpath; echo
  $TESTTMP/share/b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1/.hg

The destination should have bookmarks

  $ hg -R share-dest1a bookmarks
     bookA                     2:e5bfe23c0b47

The default path should be the remote, not the share

  $ hg -R share-dest1a config paths.default
  $TESTTMP/source1a

Clone with existing share dir should result in pull + share

  $ hg --config share.pool=share clone source1b share-dest1b
  (sharing from existing pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  adding remote bookmark branch1
  adding remote bookmark branch2
  adding remote bookmark head1
  adding remote bookmark head2
  new changesets 4a8dc1ab4c13:79168763a548
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls share
  b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1

  $ cat share-dest1b/.hg/sharedpath; echo
  $TESTTMP/share/b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1/.hg

We only get bookmarks from the remote, not everything in the share

  $ hg -R share-dest1b bookmarks
     branch1                   5:ec6257d0246c
     branch2                   6:79168763a548
     head1                     3:4a8dc1ab4c13
     head2                     4:99f71071f117

Default path should be source, not share.

  $ hg -R share-dest1b config paths.default
  $TESTTMP/source1b

Checked out revision should be head of default branch

  $ hg -R share-dest1b log -r .
  changeset:   6:79168763a548
  bookmark:    branch2
  tag:         tip
  parent:      0:b5f04eac9d8f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     branch2
  

Clone from unrelated repo should result in new share

  $ hg --config share.pool=share clone source2 share-dest2
  (sharing from new pooled repository 22aeff664783fd44c6d9b435618173c118c3448e)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 22aeff664783:63cf6c3dba4a
  searching for changes
  no changes found
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls share
  22aeff664783fd44c6d9b435618173c118c3448e
  b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1

remote naming mode works as advertised

  $ hg --config share.pool=shareremote --config share.poolnaming=remote clone source1a share-remote1a
  (sharing from new pooled repository 195bb1fcdb595c14a6c13e0269129ed78f6debde)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  new changesets b5f04eac9d8f:e5bfe23c0b47
  searching for changes
  no changes found
  adding remote bookmark bookA
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls shareremote
  195bb1fcdb595c14a6c13e0269129ed78f6debde

  $ hg --config share.pool=shareremote --config share.poolnaming=remote clone source1b share-remote1b
  (sharing from new pooled repository c0d4f83847ca2a873741feb7048a45085fd47c46)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 6 changes to 1 files
  new changesets b5f04eac9d8f:79168763a548
  searching for changes
  no changes found
  adding remote bookmark branch1
  adding remote bookmark branch2
  adding remote bookmark head1
  adding remote bookmark head2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls shareremote
  195bb1fcdb595c14a6c13e0269129ed78f6debde
  c0d4f83847ca2a873741feb7048a45085fd47c46

request to clone a single revision is respected in sharing mode

  $ hg --config share.pool=sharerevs clone -r 4a8dc1ab4c13 source1b share-1arev
  (sharing from new pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets b5f04eac9d8f:4a8dc1ab4c13
  no changes found
  adding remote bookmark head1
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R share-1arev log -G
  @  changeset:   1:4a8dc1ab4c13
  |  bookmark:    head1
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     head1
  |
  o  changeset:   0:b5f04eac9d8f
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial
  

making another clone should only pull down requested rev

  $ hg --config share.pool=sharerevs clone -r 99f71071f117 source1b share-1brev
  (sharing from existing pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark head1
  adding remote bookmark head2
  new changesets 99f71071f117
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R share-1brev log -G
  @  changeset:   2:99f71071f117
  |  bookmark:    head2
  |  tag:         tip
  |  parent:      0:b5f04eac9d8f
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     head2
  |
  | o  changeset:   1:4a8dc1ab4c13
  |/   bookmark:    head1
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     head1
  |
  o  changeset:   0:b5f04eac9d8f
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial
  

-U is respected in share clone mode

  $ hg --config share.pool=share clone -U source1a share-1anowc
  (sharing from existing pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  searching for changes
  no changes found
  adding remote bookmark bookA

  $ ls share-1anowc

Test that auto sharing doesn't cause failure of "hg clone local remote"

  $ cd $TESTTMP
  $ hg -R a id -r 0
  acb14030fe0a
  $ hg id -R remote -r 0
  abort: repository remote not found!
  [255]
  $ hg --config share.pool=share -q clone -e "\"$PYTHON\" \"$TESTDIR/dummyssh\"" a ssh://user@dummy/remote
  $ hg -R remote id -r 0
  acb14030fe0a

Cloning into pooled storage doesn't race (issue5104)

  $ HGPOSTLOCKDELAY=2.0 hg --config share.pool=racepool --config extensions.lockdelay=$TESTDIR/lockdelay.py clone source1a share-destrace1 > race1.log 2>&1 &
  $ HGPRELOCKDELAY=1.0 hg --config share.pool=racepool --config extensions.lockdelay=$TESTDIR/lockdelay.py clone source1a share-destrace2  > race2.log 2>&1
  $ wait

  $ hg -R share-destrace1 log -r tip
  changeset:   2:e5bfe23c0b47
  bookmark:    bookA
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1a
  

  $ hg -R share-destrace2 log -r tip
  changeset:   2:e5bfe23c0b47
  bookmark:    bookA
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1a
  
One repo should be new, the other should be shared from the pool. We
don't care which is which, so we just make sure we always print the
one containing "new pooled" first, then one one containing "existing
pooled".

  $ (grep 'new pooled' race1.log > /dev/null && cat race1.log || cat race2.log) | egrep -v '(lock|debugprocess)'
  (sharing from new pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  new changesets b5f04eac9d8f:e5bfe23c0b47
  searching for changes
  no changes found
  adding remote bookmark bookA
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ (grep 'existing pooled' race1.log > /dev/null && cat race1.log || cat race2.log) | egrep -v '(lock|debugprocess)'
  (sharing from existing pooled repository b5f04eac9d8f7a6a9fcb070243cccea7dc5ea0c1)
  searching for changes
  no changes found
  adding remote bookmark bookA
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

SEC: check for unsafe ssh url

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = sh -c "read l; read l; read l"
  > EOF

  $ hg clone 'ssh://-oProxyCommand=touch${IFS}owned/path'
  abort: potentially unsafe url: 'ssh://-oProxyCommand=touch${IFS}owned/path'
  [255]
  $ hg clone 'ssh://%2DoProxyCommand=touch${IFS}owned/path'
  abort: potentially unsafe url: 'ssh://-oProxyCommand=touch${IFS}owned/path'
  [255]
  $ hg clone 'ssh://fakehost|touch%20owned/path'
  abort: no suitable response from remote hg!
  [255]
  $ hg clone 'ssh://fakehost%7Ctouch%20owned/path'
  abort: no suitable response from remote hg!
  [255]

  $ hg clone 'ssh://-oProxyCommand=touch owned%20foo@example.com/nonexistent/path'
  abort: potentially unsafe url: 'ssh://-oProxyCommand=touch owned foo@example.com/nonexistent/path'
  [255]

#if windows
  $ hg clone "ssh://%26touch%20owned%20/" --debug
  running sh -c "read l; read l; read l" "&touch owned " "hg -R . serve --stdio"
  sending hello command
  sending between command
  abort: no suitable response from remote hg!
  [255]
  $ hg clone "ssh://example.com:%26touch%20owned%20/" --debug
  running sh -c "read l; read l; read l" -p "&touch owned " example.com "hg -R . serve --stdio"
  sending hello command
  sending between command
  abort: no suitable response from remote hg!
  [255]
#else
  $ hg clone "ssh://%3btouch%20owned%20/" --debug
  running sh -c "read l; read l; read l" ';touch owned ' 'hg -R . serve --stdio'
  sending hello command
  sending between command
  abort: no suitable response from remote hg!
  [255]
  $ hg clone "ssh://example.com:%3btouch%20owned%20/" --debug
  running sh -c "read l; read l; read l" -p ';touch owned ' example.com 'hg -R . serve --stdio'
  sending hello command
  sending between command
  abort: no suitable response from remote hg!
  [255]
#endif

  $ hg clone "ssh://v-alid.example.com/" --debug
  running sh -c "read l; read l; read l" v-alid\.example\.com ['"]hg -R \. serve --stdio['"] (re)
  sending hello command
  sending between command
  abort: no suitable response from remote hg!
  [255]

We should not have created a file named owned - if it exists, the
attack succeeded.
  $ if test -f owned; then echo 'you got owned'; fi

Cloning without fsmonitor enabled does not print a warning for small repos

  $ hg clone a fsmonitor-default
  updating to bookmark @
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
#else
  $ hg clone a nofsmonitor
  updating to bookmark @
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We should not see warning about fsmonitor when it is enabled

#if fsmonitor
  $ hg clone a fsmonitor-enabled
  updating to bookmark @
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

We can disable the fsmonitor warning

  $ hg --config fsmonitor.warn_when_unused=false clone a fsmonitor-disable-warning
  updating to bookmark @
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Loaded fsmonitor but disabled in config should still print warning

#if linuxormacos fsmonitor
  $ hg --config fsmonitor.mode=off clone a fsmonitor-mode-off
  updating to bookmark @
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor") (fsmonitor !)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

Warning not printed if working directory isn't empty

  $ hg -q clone a fsmonitor-update
  (warning: large working directory being used without fsmonitor enabled; enable fsmonitor to improve performance; see "hg help -e fsmonitor") (?)
  $ cd fsmonitor-update
  $ hg up acb14030fe0a
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
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

