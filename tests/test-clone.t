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

Trigger branchcache creation:

  $ hg branches
  default                       10:a7949464abda
  $ ls .hg/cache
  branch2-served
  rbc-names-v1
  rbc-revs-v1

Default operation:

  $ hg clone . ../b
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../b

Ensure branchcache got copied over:

  $ ls .hg/cache
  branch2-served

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
  linking: 1
  linking: 2
  linking: 3
  linking: 4
  linking: 5
  linking: 6
  linking: 7
  linking: 8
  linked 8 files
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

Ensure branchcache got copied over:

  $ ls .hg/cache
  branch2-served

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
  $TESTTMP/a#0 (glob)

Use --pull:

  $ hg clone --pull a g
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 11 changesets with 11 changes to 2 files
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
  $ hg branch stable
  marked working directory as branch stable
  (branches are permanent and global, did you want a bookmark?)
  $ echo some text >a
  $ hg ci -m "starting branch stable"
  $ hg tag ref2
  $ echo some more text >a
  $ hg ci -m "another change for branch stable"
  $ hg up ref2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents
  changeset:   13:e8ece76546a6
  branch:      stable
  tag:         ref2
  parent:      10:a7949464abda
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     starting branch stable
  

Repo a has two heads:

  $ hg heads
  changeset:   15:0aae7cf88f0d
  branch:      stable
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
  updating to branch stable
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:0aae7cf88f0d
  branch:      stable
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
  e8ece76546a6
  $ hg -R ua parents --template "{node|short}\n"
  e8ece76546a6

  $ rm -r ua


Testing clone --pull -u:

  $ hg clone --pull -u . a ua
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 16 changesets with 16 changes to 3 files (+1 heads)
  updating to branch stable
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:0aae7cf88f0d
  branch:      stable
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
  e8ece76546a6
  $ hg -R ua parents --template "{node|short}\n"
  e8ece76546a6

  $ rm -r ua


Testing clone -u <branch>:

  $ hg clone -u stable a ua
  updating to branch stable
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has both heads:

  $ hg -R ua heads
  changeset:   15:0aae7cf88f0d
  branch:      stable
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
  changeset:   15:0aae7cf88f0d
  branch:      stable
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
  changeset:   15:0aae7cf88f0d
  branch:      stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

Branch 'default' is checked out:

  $ hg -R ua parents
  changeset:   12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  
Test clone with a branch named "@" (issue3677)

  $ hg -R ua branch @
  marked working directory as branch @
  $ hg -R ua commit -m 'created branch @'
  $ hg clone ua atbranch
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R atbranch heads
  changeset:   16:798b6d97153e
  branch:      @
  tag:         tip
  parent:      12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     created branch @
  
  changeset:   15:0aae7cf88f0d
  branch:      stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  
  $ hg -R atbranch parents
  changeset:   12:f21241060d6a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     hacked default
  

  $ rm -r ua atbranch


Testing #<branch>:

  $ hg clone -u . a#stable ua
  adding changesets
  adding manifests
  adding file changes
  added 14 changesets with 14 changes to 3 files
  updating to branch stable
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  changeset:   13:0aae7cf88f0d
  branch:      stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   10:a7949464abda
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  e8ece76546a6
  $ hg -R ua parents --template "{node|short}\n"
  e8ece76546a6

  $ rm -r ua


Testing -u -r <branch>:

  $ hg clone -u . -r stable a ua
  adding changesets
  adding manifests
  adding file changes
  added 14 changesets with 14 changes to 3 files
  updating to branch stable
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  changeset:   13:0aae7cf88f0d
  branch:      stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   10:a7949464abda
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Same revision checked out in repo a and ua:

  $ hg -R a parents --template "{node|short}\n"
  e8ece76546a6
  $ hg -R ua parents --template "{node|short}\n"
  e8ece76546a6

  $ rm -r ua


Testing -r <branch>:

  $ hg clone -r stable a ua
  adding changesets
  adding manifests
  adding file changes
  added 14 changesets with 14 changes to 3 files
  updating to branch stable
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Repo ua has branch 'stable' and 'default' (was changed in fd511e9eeea6):

  $ hg -R ua heads
  changeset:   13:0aae7cf88f0d
  branch:      stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  
  changeset:   10:a7949464abda
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

Branch 'stable' is checked out:

  $ hg -R ua parents
  changeset:   13:0aae7cf88f0d
  branch:      stable
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another change for branch stable
  

  $ rm -r ua


Issue2267: Error in 1.6 hg.py: TypeError: 'NoneType' object is not
iterable in addbranchrevs()

  $ cat <<EOF > simpleclone.py
  > from mercurial import ui, hg
  > myui = ui.ui()
  > repo = hg.repository(myui, 'a')
  > hg.clone(myui, {}, repo, dest="ua")
  > EOF

  $ python simpleclone.py
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ rm -r ua

  $ cat <<EOF > branchclone.py
  > from mercurial import ui, hg, extensions
  > myui = ui.ui()
  > extensions.loadall(myui)
  > repo = hg.repository(myui, 'a')
  > hg.clone(myui, {}, repo, dest="ua", branch=["stable",])
  > EOF

  $ python branchclone.py
  adding changesets
  adding manifests
  adding file changes
  added 14 changesets with 14 changes to 3 files
  updating to branch stable
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
     @                         15:0aae7cf88f0d
  $ hg clone . ../i
  updating to bookmark @ on branch stable
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -i ../i
  0aae7cf88f0d
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
  $ hg clone http://127.0.0.1:3121/a b
  abort: error: * (glob)
  [255]
#else
  $ hg clone http://127.0.0.1:3121/a b
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
  $ hg -R src commit -m '#0'
  $ hg -R src log -q
  0:e1bab28bca43
  $ hg clone -U -q src dst
  $ hg -R dst log -q
  0:e1bab28bca43
  $ cd ..
