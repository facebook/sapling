Test file dedicated to testing the divergent troubles from obsolete changeset.

This is the most complex troubles from far so we isolate it in a dedicated
file.

Enable obsolete

  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate = {rev}:{node|short} {desc}\n
  > [extensions]
  > obs=${TESTTMP}/obs.py
  > [alias]
  > debugobsolete = debugobsolete -d '0 0'
  > [phases]
  > publish=False
  > EOF


  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ getid() {
  >    hg id --debug --hidden -ir "desc('$1')"
  > }

setup repo

  $ hg init reference
  $ cd reference
  $ mkcommit base
  $ mkcommit A_0
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_1
  created new head
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_2
  created new head
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cd ..


  $ newcase() {
  >    hg clone -u 0 -q reference $1
  >    cd $1
  > }

direct divergence
-----------------

A_1 have two direct and divergent successors A_1 and A_1

  $ newcase direct
  $ hg debugobsolete `getid A_0` `getid A_1`
  $ hg debugobsolete `getid A_0` `getid A_2`
  invalid branchheads cache (served): tip differs
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba
      392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg log -r 'divergent()'
  2:82623d38b9ba A_1
  3:392fd25390da A_2

check that mercurial refuse to push

  $ hg init ../other
  $ hg push ../other
  pushing to ../other
  searching for changes
  abort: push includes divergent changeset: 392fd25390da!
  [255]

  $ cd ..


indirect divergence with known changeset
-------------------------------------------

  $ newcase indirect_known
  $ hg debugobsolete `getid A_0` `getid A_1`
  $ hg debugobsolete `getid A_0` `getid A_2`
  invalid branchheads cache (served): tip differs
  $ mkcommit A_3
  created new head
  $ hg debugobsolete `getid A_2` `getid A_3`
  $ hg log -G --hidden
  @  4:01f36c5a8fda A_3
  |
  | x  3:392fd25390da A_2
  |/
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  o  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba
      01f36c5a8fda
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      01f36c5a8fda
  01f36c5a8fda
      01f36c5a8fda
  $ hg log -r 'divergent()'
  2:82623d38b9ba A_1
  4:01f36c5a8fda A_3
  $ cd ..


indirect divergence with known changeset
-------------------------------------------

  $ newcase indirect_unknown
  $ hg debugobsolete `getid A_0` aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid A_1`
  invalid branchheads cache (served): tip differs
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba
      392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg log -r 'divergent()'
  2:82623d38b9ba A_1
  3:392fd25390da A_2
  $ cd ..

do not take unknown node in account if they are final
-----------------------------------------------------

  $ newcase final-unknown
  $ hg debugobsolete `getid A_0` `getid A_1`
  $ hg debugobsolete `getid A_1` `getid A_2`
  invalid branchheads cache (served): tip differs
  $ hg debugobsolete `getid A_0` bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg debugobsolete bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccccccccccc
  $ hg debugobsolete `getid A_1` dddddddddddddddddddddddddddddddddddddddd

  $ hg debugsuccessorssets --hidden 'desc('A_0')'
  007dc284c1f8
      392fd25390da

  $ cd ..

divergence that converge again is not divergence anymore
-----------------------------------------------------

  $ newcase converged_divergence
  $ hg debugobsolete `getid A_0` `getid A_1`
  $ hg debugobsolete `getid A_0` `getid A_2`
  invalid branchheads cache (served): tip differs
  $ mkcommit A_3
  created new head
  $ hg debugobsolete `getid A_1` `getid A_3`
  $ hg debugobsolete `getid A_2` `getid A_3`
  $ hg log -G --hidden
  @  4:01f36c5a8fda A_3
  |
  | x  3:392fd25390da A_2
  |/
  | x  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  o  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda
  82623d38b9ba
      01f36c5a8fda
  392fd25390da
      01f36c5a8fda
  01f36c5a8fda
      01f36c5a8fda
  $ hg log -r 'divergent()'
  $ cd ..

split is not divergences
-----------------------------

  $ newcase split
  $ hg debugobsolete `getid A_0` `getid A_1` `getid A_2`
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba 392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg log -r 'divergent()'

Even when subsequente rewriting happen

  $ mkcommit A_3
  created new head
  $ hg debugobsolete `getid A_1` `getid A_3`
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_4
  created new head
  $ hg debugobsolete `getid A_2` `getid A_4`
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_5
  created new head
  $ hg debugobsolete `getid A_4` `getid A_5`
  $ hg log -G --hidden
  @  6:e442cfc57690 A_5
  |
  | x  5:6a411f0d7a0a A_4
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2
  |/
  | x  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  o  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda e442cfc57690
  82623d38b9ba
      01f36c5a8fda
  392fd25390da
      e442cfc57690
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      e442cfc57690
  e442cfc57690
      e442cfc57690
  $ hg log -r 'divergent()'

Check more complex obsolescence graft (with divergence)

  $ mkcommit B_0; hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg debugobsolete `getid B_0` `getid A_2`
  $ mkcommit A_7; hg up 0
  created new head
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_8; hg up 0
  created new head
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_5` `getid A_7` `getid A_8`
  $ mkcommit A_9; hg up 0
  created new head
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_5` `getid A_9`
  $ hg log -G --hidden
  o  10:bed64f5d2f5a A_9
  |
  | o  9:14608b260df8 A_8
  |/
  | o  8:7ae126973a96 A_7
  |/
  | x  7:3750ebee865d B_0
  | |
  | x  6:e442cfc57690 A_5
  |/
  | x  5:6a411f0d7a0a A_4
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2
  |/
  | x  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda bed64f5d2f5a
      01f36c5a8fda 7ae126973a96 14608b260df8
  82623d38b9ba
      01f36c5a8fda
  392fd25390da
      bed64f5d2f5a
      7ae126973a96 14608b260df8
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      bed64f5d2f5a
      7ae126973a96 14608b260df8
  e442cfc57690
      bed64f5d2f5a
      7ae126973a96 14608b260df8
  3750ebee865d
      bed64f5d2f5a
      7ae126973a96 14608b260df8
  7ae126973a96
      7ae126973a96
  14608b260df8
      14608b260df8
  bed64f5d2f5a
      bed64f5d2f5a
  $ hg log -r 'divergent()'
  4:01f36c5a8fda A_3
  8:7ae126973a96 A_7
  9:14608b260df8 A_8
  10:bed64f5d2f5a A_9

fix the divergence

  $ mkcommit A_A; hg up 0
  created new head
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_9` `getid A_A`
  $ hg debugobsolete `getid A_7` `getid A_A`
  $ hg debugobsolete `getid A_8` `getid A_A`
  $ hg log -G --hidden
  o  11:a139f71be9da A_A
  |
  | x  10:bed64f5d2f5a A_9
  |/
  | x  9:14608b260df8 A_8
  |/
  | x  8:7ae126973a96 A_7
  |/
  | x  7:3750ebee865d B_0
  | |
  | x  6:e442cfc57690 A_5
  |/
  | x  5:6a411f0d7a0a A_4
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2
  |/
  | x  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda a139f71be9da
  82623d38b9ba
      01f36c5a8fda
  392fd25390da
      a139f71be9da
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      a139f71be9da
  e442cfc57690
      a139f71be9da
  3750ebee865d
      a139f71be9da
  7ae126973a96
      a139f71be9da
  14608b260df8
      a139f71be9da
  bed64f5d2f5a
      a139f71be9da
  a139f71be9da
      a139f71be9da
  $ hg log -r 'divergent()'

  $ cd ..


Subset does not diverge
------------------------------

Do not report divergent successors-set if it is a subset of another
successors-set. (report [A,B] not [A] + [A,B])

  $ newcase subset
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ hg debugobsolete `getid A_0` `getid A_1` `getid A_2`
  invalid branchheads cache (served): tip differs
  $ hg debugsuccessorssets --hidden 'desc('A_0')'
  007dc284c1f8
      82623d38b9ba 392fd25390da

  $ cd ..
