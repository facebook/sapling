#require py2
  $ disable treemanifest

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 1},
                        read : { bytes : 1538},
                        write : { bytes : 788}}}}
  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 913},
                        write : { bytes : 641}}}}

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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 1029},
                        write : { bytes : 618}}}}

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  { metrics : { ssh : { connections : 1,
                        getpack : { calls : 1,  revs : 1},
                        read : { bytes : 625},
                        write : { bytes : 147}}}}

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
  4 files fetched over 2 fetches - (4 misses, 0.00% hit ratio) over 0.00s (?)
  added 3 changesets with 4 changes to 3 files
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 3,  revs : 3},
                        read : { bytes : 1403},
                        write : { bytes : 337}}}}

# pull from shallow to shallow (ssh)

  $ hg debugstrip -r d34c38483be9d08f205eaae60c380a29b48e0189
  $ hg pull ssh://user@dummy/$TESTTMP/shallow --config remotefilelog.cachepath=${CACHEDIR}2
  pulling from ssh://user@dummy/$TESTTMP/shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)
  added 3 changesets with 4 changes to 3 files
  remote: { metrics : { ssh : { connections : 1,
  remote:                       getpack : { calls : 1,  revs : 1},
  remote:                       read : { bytes : 625},
  remote:                       write : { bytes : 147}}}}
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 1},
                        read : { bytes : 2902},
                        write : { bytes : 765}}}}

  $ hg up
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat z
  z


  $ hg -R ../shallow debugstrip -qr 'desc(xxyy)'
  $ hg debugstrip -qr 'desc(xxyy)'
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
                        read : { bytes : 612},
                        write : { bytes : 991}}}}

  $ cd ../shallow2
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

# verify files are read-only

  $ ls_l .hg/store/packs
  -r--r--r--    1295 5feb6151e62f92e71ce2df45c52b1ee4de891c40.histidx
  -r--r--r--     350 5feb6151e62f92e71ce2df45c52b1ee4de891c40.histpack
  -r--r--r--    1117 81951d0ada20768783a3ca0ba3b2042b29cf4fd3.histidx
  -r--r--r--      90 81951d0ada20768783a3ca0ba3b2042b29cf4fd3.histpack
  -r--r--r--    1194 9b9360a408e070d43cd6a598139f56a6a358cd7c.dataidx
  -r--r--r--     253 9b9360a408e070d43cd6a598139f56a6a358cd7c.datapack
  -r--r--r--    1074 fea0528e5e8268df06cd40c39230727aff9901eb.dataidx
  -r--r--r--      63 fea0528e5e8268df06cd40c39230727aff9901eb.datapack
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
                        read : { bytes : 589},
                        write : { bytes : 1454}}}}

  $ cd ../master
  $ hg log -l 1 -r 'desc(a)' --style compact
     1489bbbc46f0   1970-01-01 00:00 +0000   test
    a
  
  $ hg up 'desc(a)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

# push public commits

  $ cd ../shallow
  $ echo p > p
  $ hg commit -qAm p
  $ hg debugmakepublic .
  $ echo d > d
  $ hg commit -qAm d

  $ cd ../shallow2
  $ hg pull ../shallow
  pulling from ../shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files

  $ cd ..

# Test pushing from shallow to shallow with multiple manifests introducing the
# same filenode. Test this by constructing two separate histories of file 'c'
# that share a file node and verifying that the history works after pushing.

  $ hginit multimf-master
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 528},
                        write : { bytes : 521}}}}
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 569},
                        write : { bytes : 521}}}}
  $ cd multimf-shallow
  $ echo a > a
  $ hg commit -qAm a
  $ echo b > b
  $ hg commit -qAm b
  $ echo c > c
  $ hg commit -qAm c1
  $ hg up -q 'desc(a)'
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
  added 5 changesets with 4 changes to 3 files
  { metrics : { ssh : { connections : 1,
                        read : { bytes : 2961},
                        write : { bytes : 618}}}}

  $ hg up -q 'desc(c22)'
  $ hg log -f -T '{rev}\n' c
  5
  4
