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
  -1: public

  $ mkcommit A

New commit are draft by default

  $ hglog
  0 1 A

Following commit are draft too

  $ mkcommit B

  $ hglog
  1 1 B
  0 1 A

Draft commit are properly created over public one:

  $ hg phase --public .
  $ hglog
  1 0 B
  0 0 A

  $ mkcommit C
  $ mkcommit D

  $ hglog
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test creating changeset as secret

  $ mkcommit E --config phases.new-commit='secret'
  $ hglog
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test the secret property is inherited

  $ mkcommit H
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
  created new head
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
  $ hg ci -m "merge B' and E"
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
  added 5 changesets with 5 changes to 5 files (+1 heads)
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

We add a secret head into the push destination.  This secreat head shadow a
visible shared between the initial repo and the push destination.

  $ hg up -q 4 # B'
  $ mkcommit Z --config phases.new-commit=secret
  $ hg phase .
  5: secret

# We now try to push a new public changeset that descend from the common public
# head shadowed by the remote secret head.

  $ cd ../initialrepo
  $ hg up -q 6 #B'
  $ mkcommit I
  created new head
  $ hg push ../push-dest
  pushing to ../push-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

:note: The "(+1 heads)" is wrong as we do not had any visible head

check that branch cache with "served" filter are properly computed and stored

  $ ls ../push-dest/.hg/cache/branchheads*
  ../push-dest/.hg/cache/branchheads-served
  $ cat ../push-dest/.hg/cache/branchheads-served
  6d6770faffce199f1fddd1cf87f6f026138cf061 6 465891ffab3c47a3c23792f7dc84156e19a90722
  b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e default
  6d6770faffce199f1fddd1cf87f6f026138cf061 default
  $ hg heads -R ../push-dest --template '{rev}:{node} {phase}\n'  #update visible cache too
  6:6d6770faffce199f1fddd1cf87f6f026138cf061 draft
  5:2713879da13d6eea1ff22b442a5a87cb31a7ce6a secret
  3:b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e draft
  $ ls ../push-dest/.hg/cache/branchheads*
  ../push-dest/.hg/cache/branchheads-served
  ../push-dest/.hg/cache/branchheads-visible
  $ cat ../push-dest/.hg/cache/branchheads-served
  6d6770faffce199f1fddd1cf87f6f026138cf061 6 465891ffab3c47a3c23792f7dc84156e19a90722
  b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e default
  6d6770faffce199f1fddd1cf87f6f026138cf061 default
  $ cat ../push-dest/.hg/cache/branchheads-visible
  6d6770faffce199f1fddd1cf87f6f026138cf061 6
  b3325c91a4d916bcc4cdc83ea3fe4ece46a42f6e default
  2713879da13d6eea1ff22b442a5a87cb31a7ce6a default
  6d6770faffce199f1fddd1cf87f6f026138cf061 default


Restore condition prior extra insertion.
  $ hg -q --config extensions.mq= strip .
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
  added 5 changesets with 5 changes to 5 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
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
  $ hglog -R clone-dest
  4 0 B'
  3 0 D
  2 0 C
  1 0 B
  0 0 A

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
  manifest:    7:5e724ffacba267b2ab726c91fc8b650710deaaa8
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
  manifest:    6:ab8bfef2392903058bf4ebb9e7746e8d7026b27a
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
  manifest:    5:5c710aa854874fe3d5fa7192e77bdb314cc08b5a
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
  manifest:    4:7173fd1c27119750b959e3a0f47ed78abe75d6dc
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
  manifest:    3:6e1f4c47ecb533ffd0c8e52cdc88afb6cd39e20c
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
  manifest:    2:66a5a01817fdf5239c273802b5b7618d051c89e4
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      C
  extra:       branch=default
  description:
  C
  
  
  changeset:   1:27547f69f25460a52fff66ad004e58da7ad3fb56
  parent:      0:4a2df7238c3b48766b5e22fafbb8a2f506ec8256
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    1:cb5cbbc1bfbf24cc34b9e8c16914e9caa2d2a7fd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      B
  extra:       branch=default
  description:
  B
  
  
  changeset:   0:4a2df7238c3b48766b5e22fafbb8a2f506ec8256
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    0:007d8c9d88841325f5c6b06371b35b4e8a2b1a83
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

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > hgext.graphlog=
  > EOF
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
  1: public
  2: draft
  3: draft
  4: secret
  7: secret


move changeset forward

(with -r option)

  $ hg phase --public -r 2
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
  $ hg phase --draft '5 or 7'
  cannot move 1 changesets to a more permissive phase, use --force
  phase changed for 1 changesets
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
  cannot move 1 changesets to a more permissive phase, use --force
  no phases changed
  [1]

  $ cd ..
