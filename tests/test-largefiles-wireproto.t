This file contains testcases that tend to be related to the wire protocol part
of largefiles.

  $ USERCACHE="$TESTTMP/cache"; export USERCACHE
  $ mkdir "${USERCACHE}"
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles=
  > purge=
  > rebase=
  > transplant=
  > [phases]
  > publish=False
  > [largefiles]
  > minsize=2
  > patterns=glob:**.dat
  > usercache=${USERCACHE}
  > [hooks]
  > precommit=sh -c "echo \\"Invoking status precommit hook\\"; hg status"
  > EOF


#if serve
vanilla clients not locked out from largefiles servers on vanilla repos
  $ mkdir r1
  $ cd r1
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cd ..
  $ hg serve -R r1 -d -p $HGPORT --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg --config extensions.largefiles=! clone http://localhost:$HGPORT r2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

largefiles clients still work with vanilla servers
  $ hg --config extensions.largefiles=! serve -R r1 -d -p $HGPORT1 --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg clone http://localhost:$HGPORT1 r3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

vanilla clients locked out from largefiles http repos
  $ mkdir r4
  $ cd r4
  $ hg init
  $ echo c1 > f1
  $ hg add --large f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cd ..

largefiles can be pushed locally (issue3583)
  $ hg init dest
  $ cd r4
  $ hg outgoing ../dest
  comparing with ../dest
  searching for changes
  changeset:   0:639881c12b4c
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m1
  
  $ hg push ../dest
  pushing to ../dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

exit code with nothing outgoing (issue3611)
  $ hg outgoing ../dest
  comparing with ../dest
  searching for changes
  no changes found
  [1]
  $ cd ..

#if serve
  $ hg serve -R r4 -d -p $HGPORT2 --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg --config extensions.largefiles=! clone http://localhost:$HGPORT2 r5
  abort: remote error:
  
  This repository uses the largefiles extension.
  
  Please enable it in your Mercurial config file.
  [255]

used all HGPORTs, kill all daemons
  $ killdaemons.py $DAEMON_PIDS
#endif

vanilla clients locked out from largefiles ssh repos
  $ hg --config extensions.largefiles=! clone -e "python \"$TESTDIR/dummyssh\"" ssh://user@dummy/r4 r5
  remote: 
  remote: This repository uses the largefiles extension.
  remote: 
  remote: Please enable it in your Mercurial config file.
  remote: 
  remote: -
  abort: remote error
  (check previous remote output)
  [255]

#if serve

largefiles clients refuse to push largefiles repos to vanilla servers
  $ mkdir r6
  $ cd r6
  $ hg init
  $ echo c1 > f1
  $ hg add f1
  $ hg commit -m "m1"
  Invoking status precommit hook
  A f1
  $ cat >> .hg/hgrc <<!
  > [web]
  > push_ssl = false
  > allow_push = *
  > !
  $ cd ..
  $ hg clone r6 r7
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd r7
  $ echo c2 > f2
  $ hg add --large f2
  $ hg commit -m "m2"
  Invoking status precommit hook
  A f2
  $ hg --config extensions.largefiles=! -R ../r6 serve -d -p $HGPORT --pid-file ../hg.pid
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ hg push http://localhost:$HGPORT
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: http://localhost:$HGPORT/ does not appear to be a largefile store
  [255]
  $ cd ..

putlfile errors are shown (issue3123)
Corrupt the cached largefile in r7 and move it out of the servers usercache
  $ mv r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8 .
  $ echo 'client side corruption' > r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8
  $ rm "$USERCACHE/4cdac4d8b084d0b599525cf732437fb337d422a8"
  $ hg init empty
  $ hg serve -R empty -d -p $HGPORT1 --pid-file hg.pid \
  >   --config 'web.allow_push=*' --config web.push_ssl=False
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg push -R r7 http://localhost:$HGPORT1
  pushing to http://localhost:$HGPORT1/
  searching for changes
  remote: largefiles: failed to put 4cdac4d8b084d0b599525cf732437fb337d422a8 into store: largefile contents do not match hash
  abort: remotestore: could not put $TESTTMP/r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8 to remote store http://localhost:$HGPORT1/ (glob)
  [255]
  $ mv 4cdac4d8b084d0b599525cf732437fb337d422a8 r7/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8
Push of file that exists on server but is corrupted - magic healing would be nice ... but too magic
  $ echo "server side corruption" > empty/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8
  $ hg push -R r7 http://localhost:$HGPORT1
  pushing to http://localhost:$HGPORT1/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files
  $ cat empty/.hg/largefiles/4cdac4d8b084d0b599525cf732437fb337d422a8
  server side corruption
  $ rm -rf empty

Push a largefiles repository to a served empty repository
  $ hg init r8
  $ echo c3 > r8/f1
  $ hg add --large r8/f1 -R r8
  $ hg commit -m "m1" -R r8
  Invoking status precommit hook
  A f1
  $ hg init empty
  $ hg serve -R empty -d -p $HGPORT2 --pid-file hg.pid \
  >   --config 'web.allow_push=*' --config web.push_ssl=False
  $ cat hg.pid >> $DAEMON_PIDS
  $ rm "${USERCACHE}"/*
  $ hg push -R r8 http://localhost:$HGPORT2/#default
  pushing to http://localhost:$HGPORT2/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ [ -f "${USERCACHE}"/02a439e5c31c526465ab1a0ca1f431f76b827b90 ]
  $ [ -f empty/.hg/largefiles/02a439e5c31c526465ab1a0ca1f431f76b827b90 ]

Clone over http, no largefiles pulled on clone.

  $ hg clone http://localhost:$HGPORT2/#default http-clone -U
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

test 'verify' with remotestore:

  $ rm "${USERCACHE}"/02a439e5c31c526465ab1a0ca1f431f76b827b90
  $ mv empty/.hg/largefiles/02a439e5c31c526465ab1a0ca1f431f76b827b90 .
  $ hg -R http-clone verify --large --lfa
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  searching 1 changesets for largefiles
  changeset 0:cf03e5bb9936: f1 missing
  verified existence of 1 revisions of 1 largefiles
  [1]
  $ mv 02a439e5c31c526465ab1a0ca1f431f76b827b90 empty/.hg/largefiles/
  $ hg -R http-clone -q verify --large --lfa

largefiles pulled on update - a largefile missing on the server:
  $ mv empty/.hg/largefiles/02a439e5c31c526465ab1a0ca1f431f76b827b90 .
  $ hg -R http-clone up --config largefiles.usercache=http-clone-usercache
  getting changed largefiles
  f1: largefile 02a439e5c31c526465ab1a0ca1f431f76b827b90 not available from http://localhost:$HGPORT2/
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R http-clone st
  ! f1
  $ hg -R http-clone up -Cqr null

largefiles pulled on update - a largefile corrupted on the server:
  $ echo corruption > empty/.hg/largefiles/02a439e5c31c526465ab1a0ca1f431f76b827b90
  $ hg -R http-clone up --config largefiles.usercache=http-clone-usercache
  getting changed largefiles
  f1: data corruption (expected 02a439e5c31c526465ab1a0ca1f431f76b827b90, got 6a7bb2556144babe3899b25e5428123735bb1e27)
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R http-clone st
  ! f1
  $ [ ! -f http-clone/.hg/largefiles/02a439e5c31c526465ab1a0ca1f431f76b827b90 ]
  $ [ ! -f http-clone/f1 ]
  $ [ ! -f http-clone-usercache ]
  $ hg -R http-clone verify --large --lfc
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  searching 1 changesets for largefiles
  verified contents of 1 revisions of 1 largefiles
  $ hg -R http-clone up -Cqr null

largefiles pulled on update - no server side problems:
  $ mv 02a439e5c31c526465ab1a0ca1f431f76b827b90 empty/.hg/largefiles/
  $ hg -R http-clone --debug up --config largefiles.usercache=http-clone-usercache --config progress.debug=true
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 000000000000, local: 000000000000+, remote: cf03e5bb9936
   .hglf/f1: remote created -> g
  getting .hglf/f1
  updating: .hglf/f1 1/1 files (100.00%)
  getting changed largefiles
  using http://localhost:$HGPORT2/
  sending capabilities command
  sending batch command
  getting largefiles: 0/1 lfile (0.00%)
  getting f1:02a439e5c31c526465ab1a0ca1f431f76b827b90
  sending getlfile command
  found 02a439e5c31c526465ab1a0ca1f431f76b827b90 in store
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls http-clone-usercache/*
  http-clone-usercache/02a439e5c31c526465ab1a0ca1f431f76b827b90

  $ rm -rf empty http-clone*

used all HGPORTs, kill all daemons
  $ killdaemons.py $DAEMON_PIDS

#endif
