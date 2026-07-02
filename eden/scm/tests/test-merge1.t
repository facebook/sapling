#require no-eden

  $ configure modernclient
  $ setconfig commands.update.check=none

  $ cat <<EOF > merge
  > from __future__ import print_function
  > import sys, os
  > 
  > try:
  >     import msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > 
  > print("merging for", os.path.basename(sys.argv[1]))
  > EOF
  $ HGMERGE="$PYTHON ../merge"; export HGMERGE

  $ newclientrepo t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"

  $ sl goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test interrupted updates by having a non-empty dir with the same name as one
of the files in a commit we're updating to

  $ mkdir b && touch b/nonempty
  $ sl up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl ci
  nothing changed
  [1]
  $ sl sum
  parent: b8bb4a988f25 
   commit #1
  commit: (clean)
  phases: 2 draft

The following line is commented out because the file doesn't exist at the moment, and some OSes error out even with `rm -f`.
$ rm b/nonempty

  $ sl up tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl sum
  parent: b8bb4a988f25 
   commit #1
  commit: (clean)
  phases: 2 draft

Prepare a basic merge

  $ sl up 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ sl add c
  $ sl commit -m "commit #2"
  $ echo This is file b1 > b
no merges expected
  $ sl merge -P b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  commit:      b8bb4a988f25
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit #1
  
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl diff --nodates
  diff -r 49035e18a8e6 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +This is file b1
  $ sl status
  M b
  $ cd ..; rm -r t

  $ newclientrepo t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"

  $ sl goto 538afb845929a25888be4211c3e2195445e26b7e
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ sl add c
  $ sl commit -m "commit #2"
  $ echo This is file b2 > b
merge should fail
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

#if symlink
symlinks to directories should be treated as regular files (issue5027)
  $ rm b
  $ ln -s 'This is file b2' b
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
symlinks shouldn't be followed
  $ rm b
  $ echo This is file b1 > .sl/b
  $ ln -s .sl/b b
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

  $ rm b
  $ echo This is file b2 > b
#endif

bad config
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a --config merge.checkunknown=x
  abort: merge.checkunknown not valid ('x' is none of 'abort', 'ignore', 'warn')
  [255]
this merge should fail
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a --config merge.checkunknown=abort
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

this merge should warn
  $ cp b b.orig
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a --config merge.checkunknown=warn
  b: replacing untracked file
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat b.orig
  This is file b2
  $ sl up --clean 'max(desc(commit))'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mv b.orig b

this merge should silently ignore
  $ cat b
  This is file b2
  $ sl merge b8bb4a988f252d4b5d47afa4be3465dbca46f10a --config merge.checkunknown=ignore
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

merge.checkignored
  $ sl up --clean b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat >> .gitignore << EOF
  > remoteignored
  > EOF
  $ echo This is file localignored3 > localignored
  $ echo This is file remoteignored3 > remoteignored
  $ sl add .gitignore localignored remoteignored
  the following files are ignored, but still added because they are explicitly specified:
    remoteignored
  (use 'sl debugignore <file>' to check why they are ignored)
  $ sl commit -m "commit #3"

  $ sl up 49035e18a8e652edd5309f18b1589e09bb4c2193
  1 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ cat >> .gitignore << EOF
  > localignored
  > EOF
  $ sl add .gitignore
  $ sl commit -m "commit #4"

remote .gitignore shouldn't be used for determining whether a file is ignored
  $ echo This is file remoteignored4 > remoteignored
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=ignore --config merge.checkunknown=abort
  remoteignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=abort --config merge.checkunknown=ignore
  merging .gitignore
  merging for .gitignore
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat .gitignore
  localignored
  $ sl status
  M .gitignore
  M b
  M localignored
  M remoteignored
  $ cat remoteignored
  This is file remoteignored3

local .gitignore should be used for that
  $ sl up --clean 'max(desc(commit))'
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo This is file localignored4 > localignored
also test other conflicting files to see we output the full set of warnings
  $ echo This is file b2 > b
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=abort --config merge.checkunknown=abort
  b: untracked file differs
  localignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=abort --config merge.checkunknown=ignore
  localignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=warn --config merge.checkunknown=abort
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ cp localignored localignored.orig
  $ cp b b.orig
  $ sl merge 6db90fb1646115381a8965d310fb1a3dddaee4a3 --config merge.checkignored=warn --config merge.checkunknown=warn
  b: replacing untracked file
  localignored: replacing untracked file
  merging .gitignore
  merging for .gitignore
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat localignored
  This is file localignored3
  $ cat localignored.orig
  This is file localignored4
  $ rm localignored.orig

  $ cat b.orig
  This is file b2
  $ sl up --clean 49035e18a8e652edd5309f18b1589e09bb4c2193
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ mv b.orig b

this merge of b should work
  $ cat b
  This is file b2
  $ sl merge -f b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  merging b
  merging for b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl diff --nodates
  diff -r 49035e18a8e6 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +This is file b2
  $ sl status
  M b
  $ cd ..; rm -r t

  $ newclientrepo t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"
  $ echo This is file b22 > b
  $ sl commit -m "commit #2"
  $ sl goto b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ sl add c
  $ sl commit -m "commit #3"

Contents of b should be "this is file b1"
  $ cat b
  This is file b1

  $ echo This is file b22 > b
merge fails
  $ sl merge 6f3a5daccd8d9e79d8992347be056c1c4c3a98fd
  abort: uncommitted changes
  (use 'sl status' to list changes)
  [255]
merge expected!
  $ sl merge -f 6f3a5daccd8d9e79d8992347be056c1c4c3a98fd
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl diff --nodates
  diff -r 85de557015a8 b
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -This is file b1
  +This is file b22
  $ sl status
  M b
  $ cd ..; rm -r t

  $ newclientrepo t
  $ echo This is file a1 > a
  $ sl add a
  $ sl commit -m "commit #0"
  $ echo This is file b1 > b
  $ sl add b
  $ sl commit -m "commit #1"
  $ echo This is file b22 > b
  $ sl commit -m "commit #2"
  $ sl goto b8bb4a988f252d4b5d47afa4be3465dbca46f10a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo This is file c1 > c
  $ sl add c
  $ sl commit -m "commit #3"
  $ echo This is file b33 > b
merge of b should fail
  $ sl merge 6f3a5daccd8d9e79d8992347be056c1c4c3a98fd
  abort: uncommitted changes
  (use 'sl status' to list changes)
  [255]
merge of b expected
  $ sl merge -f 6f3a5daccd8d9e79d8992347be056c1c4c3a98fd
  merging b
  merging for b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl diff --nodates
  diff -r 85de557015a8 b
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -This is file b1
  +This is file b33
  $ sl status
  M b

Test for issue2364

  $ sl up -qC .
  $ sl rm b
  $ sl ci -md
  $ sl revert -r tip~2 b
  $ sl up -q -- tip~2

Test that updated files are treated as "modified", when
'merge.goto()' is aborted before 'merge.recordupdates()' (= parents
aren't changed), even if none of mode, size and timestamp of them
isn't changed on the filesystem (see also issue4583).

  $ cat > $TESTTMP/abort.py <<EOF
  > from __future__ import absolute_import
  > # emulate aborting before "recordupdates()". in this case, files
  > # are changed without updating dirstate
  > from sapling import (
  >   error,
  >   extensions,
  >   merge,
  > )
  > def applyupdates(orig, *args, **kwargs):
  >     orig(*args, **kwargs)
  >     raise error.Abort('intentional aborting')
  > def extsetup(ui):
  >     extensions.wrapfunction(merge, "applyupdates", applyupdates)
  > EOF

  $ cat >> .sl/config <<EOF
  > [fakedirstatewritetime]
  > # emulate invoking dirstate.write() via repo.status()
  > # at 2000-01-01 00:00
  > fakenow = 2000-01-01 00:00:00
  > EOF

(file gotten from other revision)

  $ sl goto -q -C 6f3a5daccd8d9e79d8992347be056c1c4c3a98fd
  $ echo 'THIS IS FILE B5' > b
  $ sl commit -m 'commit #5'

  $ sl goto -q -C 85de557015a885e766d39be36993986a40acdc4d
  $ cat b
  This is file b1
  $ touch -t 200001010000 b
  $ sl debugrebuildstate

  $ cat >> .sl/config <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > abort = $TESTTMP/abort.py
  > EOF
  $ sl merge 'max(desc(commit))'
  abort: intentional aborting
  [255]
  $ cat >> .sl/config <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > abort = !
  > EOF

  $ cat b
  THIS IS FILE B5
  $ touch -t 200001010000 b
  $ sl status -A b
  M b

(file merged from other revision)

  $ sl goto -q -C 85de557015a885e766d39be36993986a40acdc4d
  $ echo 'this is file b6' > b
  $ sl commit -m 'commit #6'

  $ cat b
  this is file b6
  $ touch -t 200001010000 b
  $ sl debugrebuildstate

  $ cat >> .sl/config <<EOF
  > [extensions]
  > fakedirstatewritetime = $TESTDIR/fakedirstatewritetime.py
  > abort = $TESTTMP/abort.py
  > EOF
  $ sl merge --tool internal:other e08298e3572a6a9580c625c07288b8ccc3faffff
  abort: intentional aborting
  [255]
  $ cat >> .sl/config <<EOF
  > [extensions]
  > fakedirstatewritetime = !
  > abort = !
  > EOF

  $ cat b
  THIS IS FILE B5
  $ touch -t 200001010000 b
  $ sl status -A b
  M b

  $ cd ..
