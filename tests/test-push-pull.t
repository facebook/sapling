  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ hgcloneshallow ssh://user@dummy/master shallow2 -q

We should see the remotefilelog capability here, which advertises that
the server supports our custom getfiles method.

  $ cd master
  $ echo 'hello' | hg serve --stdio
  * (glob)
  capabilities: * remotefilelog (glob)
  $ echo 'capabilities' | hg serve --stdio ; echo
  * (glob)
  * remotefilelog (glob)

# pull to shallow from full

  $ echo y > y
  $ hg commit -qAm y

  $ cd ../shallow
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ cat y
  y

  $ cd ..

# pull from shallow to shallow (local)

  $ cd shallow
  $ echo z > z
  $ hg commit -qAm z
  $ cd ../shallow2
  $ hg pull ../shallow
  pulling from ../shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

# pull from shallow to shallow (ssh)

  $ hg strip -r 1
  saved backup bundle to $TESTTMP/shallow2/.hg/strip-backup/d34c38483be9-2e489c37-backup.hg (glob)
  $ hg pull ssh://user@dummy/$TESTTMP/shallow
  pulling from ssh://user@dummy/$TESTTMP/shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat z
  z

  $ cd ..

# push from shallow to shallow

  $ cd shallow
  $ echo a > a
  $ hg commit -qAm a
  $ hg push ssh://user@dummy/$TESTTMP/shallow2
  pushing to ssh://user@dummy/$TESTTMP/shallow2
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ cd ../shallow2
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

# verify files are g+w

  $ ls -l .hg/store/data
  total * (glob)
  drwxrwxr-x* 395df8f7c51f007019cb30201c49e884b46b92fa (glob)
  drwxrwxr-x* 86f7e437faa5a7fce15d1ddcb9eaeaea377667b8 (glob)
  $ ls -l .hg/store/data/395df8f7c51f007019cb30201c49e884b46b92fa
  total 8
  -rw-rw-r--* 69a1b67522704ec122181c0890bd16e9d3e7516a (glob)
  -rw-rw-r--* 69a1b67522704ec122181c0890bd16e9d3e7516a1 (glob)
  $ cd ..

# push from shallow to full

  $ cd shallow
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files

  $ cd ../master
  $ hg log -l 1 --style compact
  3[tip]   1489bbbc46f0   1970-01-01 00:00 +0000   test
    a
  
  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

# push public commits

  $ cd ../shallow
  $ echo p > p
  $ hg commit -qAm p
  $ hg phase -f -p -r .
  $ echo d > d
  $ hg commit -qAm d

  $ cd ../shallow2
  $ hg pull ../shallow
  pulling from ../shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

  $ cd ..

# Test pushing from shallow to shallow with multiple manifests introducing the
# same filenode. Test this by constructing two separate histories of file 'c'
# that share a file node and verifying that the history works after pushing.

  $ hginit multimf-master
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow -q
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow2 -q
  $ cd multimf-shallow
  $ echo a > a
  $ hg commit -qAm a
  $ echo b > b
  $ hg commit -qAm b
  $ echo c > c
  $ hg commit -qAm c1
  $ hg up -q 0
  $ echo c > c
  $ hg commit -qAm c2
  $ echo cc > c
  $ hg commit -qAm c22
  $ hg log -G -T '{rev} {desc}\n'
  @  4 c22
  |
  o  3 c2
  |
  | o  2 c1
  | |
  | o  1 b
  |/
  o  0 a
  

  $ cd ../multimf-shallow2
  $ hg pull ssh://user@dummy/$TESTTMP/multimf-shallow
  pulling from ssh://user@dummy/$TESTTMP/multimf-shallow
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 4 changes to 3 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg up -q 4
  $ hg log -f -T '{rev}\n' c
  4
  3
