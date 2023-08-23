  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ setconfig devel.print-metrics=1 devel.skip-metrics=scmstore,watchman

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
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)
  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

We should see the remotefilelog capability here, which advertises that
the server supports our custom getfiles method.

  $ cd master
  $ echo 'hello' | hg -R . serve --stdio
  * (glob)
  capabilities: lookup * remotefilelog getfile (glob)
  $ echo 'capabilities' | hg -R . serve --stdio ; echo
  * (glob)
  * remotefilelog getfile (glob)

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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

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
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 2,  revs : 2},
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

# pull from shallow to shallow (ssh)

  $ hg debugstrip -r d34c38483be9d08f205eaae60c380a29b48e0189
  $ hg pull ssh://user@dummy/$TESTTMP/shallow --config remotefilelog.cachepath=${CACHEDIR}2
  pulling from ssh://user@dummy/$TESTTMP/shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)
  { metrics : { ssh : { connections : 2,
                        getpack : { calls : 1,  revs : 1},
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

  $ cd ../shallow2
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a
  $ cd ..

# push from shallow to full

  $ cd shallow
  $ hg push
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

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

  $ cd ..

# Test pushing from shallow to shallow with multiple manifests introducing the
# same filenode. Test this by constructing two separate histories of file 'c'
# that share a file node and verifying that the history works after pushing.

  $ hginit multimf-master
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)
  $ hgcloneshallow ssh://user@dummy/multimf-master multimf-shallow2 -q
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)
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
  $ hg log -G -T '{desc}\n'
  @  c22
  │
  o  c2
  │
  │ o  c1
  │ │
  │ o  b
  ├─╯
  o  a
  

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
  { metrics : { ssh : { connections : 1,
                        read : { bytes : *}, (glob)
                        write : { bytes : *}}}} (glob)

  $ hg up -q 'desc(c22)'
  $ hg log -f -T '{node}\n' c
  d8f06a4c6d38c9308d3dcbee10c27ae5a01ab93f
  853b3dc5bcf912b088c60ccd3ec60f35e96b92bb
