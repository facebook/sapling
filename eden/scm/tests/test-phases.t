  $ setconfig extensions.treemanifest=!
  $ cat > $TESTTMP/hook.sh << 'EOF'
  > echo "test-hook-close-phase: $HG_NODE:  $HG_OLDPHASE -> $HG_PHASE"
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > phasereport=$TESTDIR/testlib/ext-phase-report.py
  > [hooks]
  > txnclose-phase.test = sh $TESTTMP/hook.sh
  > EOF

  $ hglog() { hg log --template "{rev} {phaseidx} {desc}\n" $*; }
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    message="$1"
  >    shift
  >    hg ci -m "$message" $*
  > }

  $ hg init initialrepo
  $ cd initialrepo

Cannot change null revision phase

  $ hg phase --force --secret null
  abort: cannot change null revision phase
  [255]
  $ hg phase null
  0000000000000000000000000000000000000000: public

  $ mkcommit A
  test-debug-phase: new rev 0:  x -> 1
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:   -> draft

New commit are draft by default

  $ hglog
  0 1 A

Following commit are draft too

  $ mkcommit B
  test-debug-phase: new rev 1:  x -> 1
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:   -> draft

  $ hglog
  1 1 B
  0 1 A

Draft commit are properly created over public one:

  $ hg phase --public .
  test-debug-phase: move rev 0: 1 -> 0
  test-debug-phase: move rev 1: 1 -> 0
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:  draft -> public
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:  draft -> public
  $ hg phase
  27547f69f25460a52fff66ad004e58da7ad3fb56: public
  $ hglog
  1 0 B
  0 0 A

  $ mkcommit C
  test-debug-phase: new rev 2:  x -> 1
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:   -> draft
  $ mkcommit D
  test-debug-phase: new rev 3:  x -> 1
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:   -> draft

  $ hglog
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test creating changeset as secret

  $ mkcommit E --config phases.new-commit='secret'
  test-debug-phase: new rev 4:  x -> 2
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:   -> secret
  $ hglog
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test the secret property is inherited

  $ mkcommit H
  test-debug-phase: new rev 5:  x -> 2
  test-hook-close-phase: a030c6be5127abc010fcbff1851536552e6951a8:   -> secret
  $ hglog
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Even on merge

  $ hg up -q 1
  $ mkcommit "B'"
  test-debug-phase: new rev 6:  x -> 1
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:   -> draft
  $ hglog
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A
  $ hg merge 4 # E
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg phase
  cf9fe039dfd67e829edf6522a45de057b5c86519: draft
  a603bfb5a83e312131cebcd05353c217d4d21dde: secret
  $ hg ci -m "merge B' and E"
  test-debug-phase: new rev 7:  x -> 2
  test-hook-close-phase: 17a481b3bccb796c0521ae97903d81c52bfee4af:   -> secret

  $ hglog
  7 2 merge B' and E
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test secret changeset are not pushed

  $ hg init ../push-dest
  $ cat > ../push-dest/.hg/hgrc << EOF
  > [phases]
  > publish=False
  > EOF
  $ hg outgoing ../push-dest --template='{rev} {phase} {desc|firstline}\n'
  comparing with ../push-dest
  searching for changes
  0 public A
  1 public B
  2 draft C
  3 draft D
  6 draft B'
  $ hg outgoing -r 'branch(default)' ../push-dest --template='{rev} {phase} {desc|firstline}\n'
  comparing with ../push-dest
  searching for changes
  0 public A
  1 public B
  2 draft C
  3 draft D
  6 draft B'

  $ hg push ../push-dest -f # force because we push multiple heads
  pushing to ../push-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 1
  test-debug-phase: new rev 3:  x -> 1
  test-debug-phase: new rev 4:  x -> 1
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:   -> public
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:   -> public
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:   -> draft
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:   -> draft
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:   -> draft
  $ hglog
  7 2 merge B' and E
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A
  $ cd ../push-dest
  $ hglog
  4 1 B'
  3 1 D
  2 1 C
  1 0 B
  0 0 A

(Issue3303)
Check that remote secret changeset are ignore when checking creation of remote heads

We add a secret head into the push destination. This secret head shadows a
visible shared between the initial repo and the push destination.

  $ hg up -q 4 # B'
  $ mkcommit Z --config phases.new-commit=secret
  test-debug-phase: new rev 5:  x -> 2
  test-hook-close-phase: 2713879da13d6eea1ff22b442a5a87cb31a7ce6a:   -> secret
  $ hg phase .
  2713879da13d6eea1ff22b442a5a87cb31a7ce6a: secret

We now try to push a new public changeset that descend from the common public
head shadowed by the remote secret head.

  $ cd ../initialrepo
  $ hg up -q 6 #B'
  $ mkcommit I
  test-debug-phase: new rev 8:  x -> 1
  test-hook-close-phase: 6d6770faffce199f1fddd1cf87f6f026138cf061:   -> draft
  $ hg push ../push-dest
  pushing to ../push-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  test-debug-phase: new rev 6:  x -> 1
  test-hook-close-phase: 6d6770faffce199f1fddd1cf87f6f026138cf061:   -> draft

:note: The "(+1 heads)" is wrong as we do not had any visible head

Restore condition prior extra insertion.
  $ hg -q debugstrip .
  $ hg up -q 7
  $ cd ..

Test secret changeset are not pull

  $ hg init pull-dest
  $ cd pull-dest
  $ hg pull ../initialrepo
  pulling from ../initialrepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files
  new changesets 4a2df7238c3b:cf9fe039dfd6
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:   -> public
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:   -> public
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:   -> public
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:   -> public
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:   -> public
  $ hglog
  4 0 B'
  3 0 D
  2 0 C
  1 0 B
  0 0 A
  $ cd ..

But secret can still be bundled explicitly

  $ cd initialrepo
  $ hg bundle --base '4^' -r 'children(4)' ../secret-bundle.hg
  4 changesets found
  $ cd ..

Test secret changeset are not cloned
(during local clone)

  $ hg clone -qU initialrepo clone-dest
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:   -> public
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:   -> public
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:   -> public
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:   -> public
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:   -> public
  $ hglog -R clone-dest
  4 0 B'
  3 0 D
  2 0 C
  1 0 B
  0 0 A

Test summary

  $ hg summary -R clone-dest --verbose
  parent: -1:000000000000  (no revision checked out)
  commit: (clean)
  $ hg summary -R initialrepo
  parent: 7:17a481b3bccb tip
   merge B' and E
  commit: (clean) (secret)
  phases: 3 draft, 3 secret
  $ hg summary -R initialrepo --quiet
  parent: 7:17a481b3bccb tip

Test revset

  $ cd initialrepo
  $ hglog -r 'public()'
  0 0 A
  1 0 B
  $ hglog -r 'draft()'
  2 1 C
  3 1 D
  6 1 B'
  $ hglog -r 'secret()'
  4 2 E
  5 2 H
  7 2 merge B' and E

test that phase are displayed in log at debug level

  $ hg log --debug
  changeset:   7:17a481b3bccb796c0521ae97903d81c52bfee4af
  tag:         tip
  phase:       secret
  parent:      6:cf9fe039dfd67e829edf6522a45de057b5c86519
  parent:      4:a603bfb5a83e312131cebcd05353c217d4d21dde
  manifest:    5e724ffacba267b2ab726c91fc8b650710deaaa8
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      C D E
  extra:       branch=default
  description:
  merge B' and E
  
  
  changeset:   6:cf9fe039dfd67e829edf6522a45de057b5c86519
  phase:       draft
  parent:      1:27547f69f25460a52fff66ad004e58da7ad3fb56
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    ab8bfef2392903058bf4ebb9e7746e8d7026b27a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      B'
  extra:       branch=default
  description:
  B'
  
  
  changeset:   5:a030c6be5127abc010fcbff1851536552e6951a8
  phase:       secret
  parent:      4:a603bfb5a83e312131cebcd05353c217d4d21dde
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    5c710aa854874fe3d5fa7192e77bdb314cc08b5a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      H
  extra:       branch=default
  description:
  H
  
  
  changeset:   4:a603bfb5a83e312131cebcd05353c217d4d21dde
  phase:       secret
  parent:      3:b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    7173fd1c27119750b959e3a0f47ed78abe75d6dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      E
  extra:       branch=default
  description:
  E
  
  
  changeset:   3:b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e
  phase:       draft
  parent:      2:f838bfaca5c7226600ebcfd84f3c3c13a28d3757
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6e1f4c47ecb533ffd0c8e52cdc88afb6cd39e20c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      D
  extra:       branch=default
  description:
  D
  
  
  changeset:   2:f838bfaca5c7226600ebcfd84f3c3c13a28d3757
  phase:       draft
  parent:      1:27547f69f25460a52fff66ad004e58da7ad3fb56
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    66a5a01817fdf5239c273802b5b7618d051c89e4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      C
  extra:       branch=default
  description:
  C
  
  
  changeset:   1:27547f69f25460a52fff66ad004e58da7ad3fb56
  phase:       public
  parent:      0:4a2df7238c3b48766b5e22fafbb8a2f506ec8256
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    cb5cbbc1bfbf24cc34b9e8c16914e9caa2d2a7fd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      B
  extra:       branch=default
  description:
  B
  
  
  changeset:   0:4a2df7238c3b48766b5e22fafbb8a2f506ec8256
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    007d8c9d88841325f5c6b06371b35b4e8a2b1a83
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      A
  extra:       branch=default
  description:
  A
  
  


(Issue3707)
test invalid phase name

  $ mkcommit I --config phases.new-commit='babar'
  transaction abort!
  rollback completed
  abort: phases.new-commit: not a valid phase name ('babar')
  [255]
Test phase command
===================

initial picture

  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 public B
  |
  o  0 public A
  

display changesets phase

(mixing -r and plain rev specification)

  $ hg phase 1::4 -r 7
  27547f69f25460a52fff66ad004e58da7ad3fb56: public
  f838bfaca5c7226600ebcfd84f3c3c13a28d3757: draft
  b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e: draft
  a603bfb5a83e312131cebcd05353c217d4d21dde: secret
  17a481b3bccb796c0521ae97903d81c52bfee4af: secret


move changeset forward

(with -r option)

  $ hg phase --public -r 2
  test-debug-phase: move rev 2: 1 -> 0
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:  draft -> public
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 public C
  |/
  o  1 public B
  |
  o  0 public A
  

move changeset backward

(without -r option)

  $ hg phase --draft --force 2
  test-debug-phase: move rev 2: 0 -> 1
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:  public -> draft
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 public B
  |
  o  0 public A
  

move changeset forward and backward

  $ hg phase --draft --force 1::4
  test-debug-phase: move rev 1: 0 -> 1
  test-debug-phase: move rev 4: 2 -> 1
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:  public -> draft
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:  secret -> draft
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 draft E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 draft B
  |
  o  0 public A
  
test partial failure

  $ hg phase --public 7
  test-debug-phase: move rev 1: 1 -> 0
  test-debug-phase: move rev 2: 1 -> 0
  test-debug-phase: move rev 3: 1 -> 0
  test-debug-phase: move rev 4: 1 -> 0
  test-debug-phase: move rev 6: 1 -> 0
  test-debug-phase: move rev 7: 2 -> 0
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:  draft -> public
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:  draft -> public
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:  draft -> public
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:  draft -> public
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:  draft -> public
  test-hook-close-phase: 17a481b3bccb796c0521ae97903d81c52bfee4af:  secret -> public
  $ hg phase --draft '5 or 7'
  test-debug-phase: move rev 5: 2 -> 1
  test-hook-close-phase: a030c6be5127abc010fcbff1851536552e6951a8:  secret -> draft
  cannot move 1 changesets to a higher phase, use --force
  [1]
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 public merge B' and E
  |\
  | o  6 public B'
  | |
  +---o  5 draft H
  | |
  o |  4 public E
  | |
  o |  3 public D
  | |
  o |  2 public C
  |/
  o  1 public B
  |
  o  0 public A
  

test complete failure

  $ hg phase --draft 7
  cannot move 1 changesets to a higher phase, use --force
  [1]

  $ cd ..

test hidden changeset are not cloned as public (issue3935)

  $ cd initialrepo

(enabling evolution)
  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution.createmarkers=True
  > EOF

(making a changeset hidden; H in that case)
  $ hg debugobsolete `hg id --debug -r 5`
  obsoleted 1 changesets

  $ cd ..
  $ hg clone initialrepo clonewithobs
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 7 changesets with 6 changes to 6 files
  new changesets 4a2df7238c3b:17a481b3bccb
  test-debug-phase: new rev 0:  x -> 0
  test-debug-phase: new rev 1:  x -> 0
  test-debug-phase: new rev 2:  x -> 0
  test-debug-phase: new rev 3:  x -> 0
  test-debug-phase: new rev 4:  x -> 0
  test-debug-phase: new rev 5:  x -> 0
  test-debug-phase: new rev 6:  x -> 0
  test-hook-close-phase: 4a2df7238c3b48766b5e22fafbb8a2f506ec8256:   -> public
  test-hook-close-phase: 27547f69f25460a52fff66ad004e58da7ad3fb56:   -> public
  test-hook-close-phase: f838bfaca5c7226600ebcfd84f3c3c13a28d3757:   -> public
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:   -> public
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:   -> public
  test-hook-close-phase: cf9fe039dfd67e829edf6522a45de057b5c86519:   -> public
  test-hook-close-phase: 17a481b3bccb796c0521ae97903d81c52bfee4af:   -> public
  updating to branch default
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clonewithobs
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    6 public merge B' and E
  |\
  | o  5 public B'
  | |
  o |  4 public E
  | |
  o |  3 public D
  | |
  o |  2 public C
  |/
  o  1 public B
  |
  o  0 public A
  

test verify repo containing hidden changesets, which should not abort just
because repo.cancopy() is False

  $ cd ../initialrepo
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  7 files, 8 changesets, 7 total revisions

  $ cd ..

check whether HG_PENDING makes pending changes only in related
repositories visible to an external hook.

(emulate a transaction running concurrently by copied
.hg/phaseroots.pending in subsequent test)

  $ cat > $TESTTMP/savepending.sh <<EOF
  > cp .hg/store/phaseroots.pending  .hg/store/phaseroots.pending.saved
  > exit 1 # to avoid changing phase for subsequent tests
  > EOF
  $ cd push-dest
  $ hg phase 6
  6d6770faffce199f1fddd1cf87f6f026138cf061: draft
  $ hg --config hooks.pretxnclose="sh $TESTTMP/savepending.sh" phase -f -s 6
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]
  $ cp .hg/store/phaseroots.pending.saved .hg/store/phaseroots.pending

(check (in)visibility of phaseroot while transaction running in repo)

  $ cat > $TESTTMP/checkpending.sh <<EOF
  > echo '@initialrepo'
  > hg -R "$TESTTMP/initialrepo" phase 7
  > echo '@push-dest'
  > hg -R "$TESTTMP/push-dest" phase 6
  > exit 1 # to avoid changing phase for subsequent tests
  > EOF
  $ cd ../initialrepo
  $ hg phase 7
  17a481b3bccb796c0521ae97903d81c52bfee4af: public
  $ hg --config hooks.pretxnclose="sh $TESTTMP/checkpending.sh" phase -f -s 7
  @initialrepo
  17a481b3bccb796c0521ae97903d81c52bfee4af: secret
  @push-dest
  6d6770faffce199f1fddd1cf87f6f026138cf061: draft
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]

Check that pretxnclose-phase hook can control phase movement

  $ hg phase --force b3325c91a4d9 --secret
  test-debug-phase: move rev 3: 0 -> 2
  test-debug-phase: move rev 4: 0 -> 2
  test-debug-phase: move rev 5: 1 -> 2
  test-debug-phase: move rev 7: 0 -> 2
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:  public -> secret
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:  public -> secret
  test-hook-close-phase: a030c6be5127abc010fcbff1851536552e6951a8:  draft -> secret
  test-hook-close-phase: 17a481b3bccb796c0521ae97903d81c52bfee4af:  public -> secret
  $ hg log -G -T phases
  @    changeset:   7:17a481b3bccb
  |\   tag:         tip
  | |  phase:       secret
  | |  parent:      6:cf9fe039dfd6
  | |  parent:      4:a603bfb5a83e
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge B' and E
  | |
  | o  changeset:   6:cf9fe039dfd6
  | |  phase:       public
  | |  parent:      1:27547f69f254
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     B'
  | |
  o |  changeset:   4:a603bfb5a83e
  | |  phase:       secret
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     E
  | |
  o |  changeset:   3:b3325c91a4d9
  | |  phase:       secret
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     D
  | |
  o |  changeset:   2:f838bfaca5c7
  |/   phase:       public
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     C
  |
  o  changeset:   1:27547f69f254
  |  phase:       public
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B
  |
  o  changeset:   0:4a2df7238c3b
     phase:       public
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  

Install a hook that prevent b3325c91a4d9 to become public

  $ cat >> .hg/hgrc << EOF
  > [hooks]
  > pretxnclose-phase.nopublish_D = sh -c "(echo \$HG_NODE| grep -v b3325c91a4d9>/dev/null) || [ 'public' != \$HG_PHASE ]"
  > EOF

Try various actions. only the draft move should succeed

  $ hg phase --public b3325c91a4d9
  transaction abort!
  rollback completed
  abort: pretxnclose-phase.nopublish_D hook exited with status 1
  [255]
  $ hg phase --public a603bfb5a83e
  transaction abort!
  rollback completed
  abort: pretxnclose-phase.nopublish_D hook exited with status 1
  [255]
  $ hg phase --draft 17a481b3bccb
  test-debug-phase: move rev 3: 2 -> 1
  test-debug-phase: move rev 4: 2 -> 1
  test-debug-phase: move rev 7: 2 -> 1
  test-hook-close-phase: b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e:  secret -> draft
  test-hook-close-phase: a603bfb5a83e312131cebcd05353c217d4d21dde:  secret -> draft
  test-hook-close-phase: 17a481b3bccb796c0521ae97903d81c52bfee4af:  secret -> draft
  $ hg phase --public 17a481b3bccb
  transaction abort!
  rollback completed
  abort: pretxnclose-phase.nopublish_D hook exited with status 1
  [255]
