  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"
  $ setconfig devel.print-metrics=1

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
  { metrics : { ssh : { connections : 2,
                        getfiles : { calls : 1,  revs : 1},
                        read : { bytes : 1462},
                        write : { bytes : 812}}}}
  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 918},
                        write : { bytes : 656}}}}

We should see the remotefilelog capability here, which advertises that
the server supports our custom getfiles method.

  $ cd master
  $ echo 'hello' | hg -R . serve --stdio
  * (glob)
  capabilities: lookup * remotefilelog getflogheads getfile (glob)
  $ echo 'capabilities' | hg -R . serve --stdio ; echo
  * (glob)
  * remotefilelog getflogheads getfile (glob)

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
  new changesets d34c38483be9
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 1034},
                        write : { bytes : 633}}}}

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  { metrics : { ssh : { connections : 1,
                        getfiles : { calls : 1,  revs : 1},
                        read : { bytes : 544},
                        write : { bytes : 156}}}}

  $ cat y
  y

  $ cd ..

# pull from shallow to shallow (local)

  $ cd shallow
  $ echo z > z
  $ hg commit -qAm z
  $ echo x >> x
  $ echo y >> y
  $ hg commit -qAm xxyy
  $ cd ../shallow2
  $ clearcache
  $ hg pull ../shallow
  pulling from ../shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  new changesets d34c38483be9:d7373980d475
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  { metrics : { ssh : { connections : 1,
                        getfiles : { calls : 1,  revs : 2},
                        read : { bytes : 611},
                        write : { bytes : 198}}}}

# pull from shallow to shallow (ssh)

  $ hg debugstrip -r 1
  saved backup bundle to $TESTTMP/shallow2/.hg/strip-backup/d34c38483be9-89d325c9-backup.hg (glob)
  $ hg pull ssh://user@dummy/$TESTTMP/shallow --config remotefilelog.cachepath=${CACHEDIR}2
  pulling from ssh://user@dummy/$TESTTMP/shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  new changesets d34c38483be9:d7373980d475
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  { metrics : { ssh : { connections : 2,
                        getfiles : { calls : 1,  revs : 2},
                        read : { bytes : 2793},
                        write : { bytes : 831}}}}

  $ hg up
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat z
  z


  $ hg -R ../shallow debugstrip -qr 3
  $ hg debugstrip -qr 3
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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 707},
                        write : { bytes : 1058}}}}

  $ cd ../shallow2
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

# verify files are read-only

  $ ls -l .hg/store/data
  total * (glob)
  drwxrwxr-x* 11f6ad8ec52a2984abaafd7c3b516503785c2072 (glob)
  drwxrwxr-x* 395df8f7c51f007019cb30201c49e884b46b92fa (glob)
  drwxrwxr-x* 86f7e437faa5a7fce15d1ddcb9eaeaea377667b8 (glob)
  drwxrwxr-x* 95cb0bfd2977c761298d9624e4b4d4c72a39974a (glob)
  $ ls -l .hg/store/data/395df8f7c51f007019cb30201c49e884b46b92fa
  total * (glob)
  -r--r--r--* 69a1b67522704ec122181c0890bd16e9d3e7516a (glob)
  -r--r--r--* 69a1b67522704ec122181c0890bd16e9d3e7516a_old (glob)
  -r--r--r--* filename (glob)
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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 673},
                        write : { bytes : 1575}}}}

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
  new changesets 3a2e32c04641:cedeb4167c1f

  $ cd ..

# Test pushing from shallow to shallow with multiple manifests introducing the
# same filenode. Test this by constructing two separate histories of file 'c'
# that share a file node and verifying that the history works after pushing.

  $ hginit multimf-master
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 574},
                        write : { bytes : 577}}}}
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 574},
                        write : { bytes : 577}}}}
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
- initial commit to prevent hg pull from being a clone
  $ echo z > z && hg commit -qAm z
  $ hg pull -f ssh://user@dummy/$TESTTMP/multimf-shallow
  pulling from ssh://user@dummy/$TESTTMP/multimf-shallow
  searching for changes
  warning: repository is unrelated
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 4 changes to 3 files (+2 heads)
  new changesets cb9a9f314b8b:d8f06a4c6d38
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 2966},
                        write : { bytes : 674}}}}

  $ hg up -q 5
  $ hg log -f -T '{rev}\n' c
  5
  4
