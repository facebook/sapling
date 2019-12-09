#chg-compatible

  $ setconfig extensions.treemanifest=!
Test file dedicated to testing the divergent troubles from obsolete changeset.

This is the most complex troubles from far so we isolate it in a dedicated
file.

Enable obsolete

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > logtemplate = {rev}:{node|short} {desc}{if(obsfate, " [{join(obsfate, "; ")}]")}\n
  > [experimental]
  > evolution.createmarkers=True
  > [extensions]
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
  >    hg log --hidden -r "desc('$1')" -T '{node}\n'
  > }

setup repo

  $ hg init reference
  $ cd reference
  $ mkcommit base
  $ mkcommit A_0
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_1
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_2
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
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0 [rewritten as 2:82623d38b9ba; rewritten as 3:392fd25390da]
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg log -r 'contentdivergent()'
  2:82623d38b9ba A_1
  3:392fd25390da A_2
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da

check that mercurial refuse to push

  $ hg init ../other
  $ hg push ../other
  pushing to ../other
  searching for changes
  abort: push includes content-divergent changeset: 392fd25390da!
  [255]

  $ cd ..


indirect divergence with known changeset
-------------------------------------------

  $ newcase indirect_known
  $ hg debugobsolete `getid A_0` `getid A_1`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ mkcommit A_3
  $ hg debugobsolete `getid A_2` `getid A_3`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  4:01f36c5a8fda A_3
  |
  | x  3:392fd25390da A_2 [rewritten as 4:01f36c5a8fda]
  |/
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0 [rewritten as 2:82623d38b9ba; rewritten as 3:392fd25390da]
  |/
  o  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      01f36c5a8fda
  01f36c5a8fda
      01f36c5a8fda
  $ hg log -r 'contentdivergent()'
  2:82623d38b9ba A_1
  4:01f36c5a8fda A_3
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  82623d38b9ba
      82623d38b9ba
  01f36c5a8fda
      01f36c5a8fda
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  01f36c5a8fda
      01f36c5a8fda
  $ cd ..


indirect divergence with known changeset
-------------------------------------------

  $ newcase indirect_unknown
  $ hg debugobsolete `getid A_0` aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  obsoleted 1 changesets
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa `getid A_1`
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0 [rewritten as 2:82623d38b9ba; rewritten as 3:392fd25390da]
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg log -r 'contentdivergent()'
  2:82623d38b9ba A_1
  3:392fd25390da A_2
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ cd ..

do not take unknown node in account if they are final
-----------------------------------------------------

  $ newcase final-unknown
  $ hg debugobsolete `getid A_0` `getid A_1`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_1` `getid A_2`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_0` bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg debugobsolete bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb cccccccccccccccccccccccccccccccccccccccc
  $ hg debugobsolete `getid A_1` dddddddddddddddddddddddddddddddddddddddd

  $ hg debugsuccessorssets --hidden 'desc('A_0')'
  007dc284c1f8
      392fd25390da
  $ hg debugsuccessorssets 'desc('A_0')' --closest
  $ hg debugsuccessorssets 'desc('A_0')' --closest --hidden
  007dc284c1f8
      82623d38b9ba

  $ cd ..

divergence that converge again is not divergence anymore
-----------------------------------------------------

  $ newcase converged_divergence
  $ hg debugobsolete `getid A_0` `getid A_1`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_0` `getid A_2`
  $ mkcommit A_3
  $ hg debugobsolete `getid A_1` `getid A_3`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_2` `getid A_3`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  4:01f36c5a8fda A_3
  |
  | x  3:392fd25390da A_2 [rewritten as 4:01f36c5a8fda]
  |/
  | x  2:82623d38b9ba A_1 [rewritten as 4:01f36c5a8fda]
  |/
  | x  1:007dc284c1f8 A_0 [rewritten as 2:82623d38b9ba; rewritten as 3:392fd25390da]
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
  $ hg log -r 'contentdivergent()'
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  01f36c5a8fda
      01f36c5a8fda
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      392fd25390da
      82623d38b9ba
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  01f36c5a8fda
      01f36c5a8fda
  $ cd ..

split is not divergences
-----------------------------

  $ newcase split
  $ hg debugobsolete `getid A_0` `getid A_1` `getid A_2`
  obsoleted 1 changesets
  $ hg log -G --hidden
  o  3:392fd25390da A_2
  |
  | o  2:82623d38b9ba A_1
  |/
  | x  1:007dc284c1f8 A_0 [split as 2:82623d38b9ba, 3:392fd25390da]
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
  $ hg log -r 'contentdivergent()'
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba 392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da

Even when subsequent rewriting happen

  $ mkcommit A_3
  $ hg debugobsolete `getid A_1` `getid A_3`
  obsoleted 1 changesets
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_4
  $ hg debugobsolete `getid A_2` `getid A_4`
  obsoleted 1 changesets
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_5
  $ hg debugobsolete `getid A_4` `getid A_5`
  obsoleted 1 changesets
  $ hg log -G --hidden
  @  6:e442cfc57690 A_5
  |
  | x  5:6a411f0d7a0a A_4 [rewritten as 6:e442cfc57690]
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2 [rewritten as 5:6a411f0d7a0a]
  |/
  | x  2:82623d38b9ba A_1 [rewritten as 4:01f36c5a8fda]
  |/
  | x  1:007dc284c1f8 A_0 [split as 2:82623d38b9ba, 3:392fd25390da]
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
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  01f36c5a8fda
      01f36c5a8fda
  e442cfc57690
      e442cfc57690
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba 392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      e442cfc57690
  e442cfc57690
      e442cfc57690
  $ hg log -r 'contentdivergent()'

Check more complex obsolescence graft (with divergence)

  $ mkcommit B_0; hg up 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg debugobsolete `getid B_0` `getid A_2`
  obsoleted 1 changesets
  $ mkcommit A_7; hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit A_8; hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_5` `getid A_7` `getid A_8`
  obsoleted 1 changesets
  $ mkcommit A_9; hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_5` `getid A_9`
  $ hg log -G --hidden
  o  10:bed64f5d2f5a A_9
  |
  | o  9:14608b260df8 A_8
  |/
  | o  8:7ae126973a96 A_7
  |/
  | x  7:3750ebee865d B_0 [rewritten as 3:392fd25390da]
  | |
  | x  6:e442cfc57690 A_5 [rewritten as 10:bed64f5d2f5a; split as 8:7ae126973a96, 9:14608b260df8]
  |/
  | x  5:6a411f0d7a0a A_4 [rewritten as 6:e442cfc57690]
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2 [rewritten as 5:6a411f0d7a0a]
  |/
  | x  2:82623d38b9ba A_1 [rewritten as 4:01f36c5a8fda]
  |/
  | x  1:007dc284c1f8 A_0 [split as 2:82623d38b9ba, 3:392fd25390da]
  |/
  @  0:d20a80d4def3 base
  
  $ hg debugsuccessorssets --hidden 'all()'
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      01f36c5a8fda 7ae126973a96 14608b260df8
      01f36c5a8fda bed64f5d2f5a
  82623d38b9ba
      01f36c5a8fda
  392fd25390da
      7ae126973a96 14608b260df8
      bed64f5d2f5a
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      7ae126973a96 14608b260df8
      bed64f5d2f5a
  e442cfc57690
      7ae126973a96 14608b260df8
      bed64f5d2f5a
  3750ebee865d
      7ae126973a96 14608b260df8
      bed64f5d2f5a
  7ae126973a96
      7ae126973a96
  14608b260df8
      14608b260df8
  bed64f5d2f5a
      bed64f5d2f5a
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  01f36c5a8fda
      01f36c5a8fda
  7ae126973a96
      7ae126973a96
  14608b260df8
      14608b260df8
  bed64f5d2f5a
      bed64f5d2f5a
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba 392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      e442cfc57690
  e442cfc57690
      e442cfc57690
  3750ebee865d
      392fd25390da
  7ae126973a96
      7ae126973a96
  14608b260df8
      14608b260df8
  bed64f5d2f5a
      bed64f5d2f5a
  $ hg log -r 'contentdivergent()'
  4:01f36c5a8fda A_3
  8:7ae126973a96 A_7
  9:14608b260df8 A_8
  10:bed64f5d2f5a A_9

fix the divergence

  $ mkcommit A_A; hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg debugobsolete `getid A_9` `getid A_A`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_7` `getid A_A`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_8` `getid A_A`
  obsoleted 1 changesets
  $ hg log -G --hidden
  o  11:a139f71be9da A_A
  |
  | x  10:bed64f5d2f5a A_9 [rewritten as 11:a139f71be9da]
  |/
  | x  9:14608b260df8 A_8 [rewritten as 11:a139f71be9da]
  |/
  | x  8:7ae126973a96 A_7 [rewritten as 11:a139f71be9da]
  |/
  | x  7:3750ebee865d B_0 [rewritten as 3:392fd25390da]
  | |
  | x  6:e442cfc57690 A_5 [rewritten as 10:bed64f5d2f5a; split as 8:7ae126973a96, 9:14608b260df8]
  |/
  | x  5:6a411f0d7a0a A_4 [rewritten as 6:e442cfc57690]
  |/
  | o  4:01f36c5a8fda A_3
  |/
  | x  3:392fd25390da A_2 [rewritten as 5:6a411f0d7a0a]
  |/
  | x  2:82623d38b9ba A_1 [rewritten as 4:01f36c5a8fda]
  |/
  | x  1:007dc284c1f8 A_0 [split as 2:82623d38b9ba, 3:392fd25390da]
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
  $ hg debugsuccessorssets 'all()' --closest
  d20a80d4def3
      d20a80d4def3
  01f36c5a8fda
      01f36c5a8fda
  a139f71be9da
      a139f71be9da
  $ hg debugsuccessorssets 'all()' --closest --hidden
  d20a80d4def3
      d20a80d4def3
  007dc284c1f8
      82623d38b9ba 392fd25390da
  82623d38b9ba
      82623d38b9ba
  392fd25390da
      392fd25390da
  01f36c5a8fda
      01f36c5a8fda
  6a411f0d7a0a
      e442cfc57690
  e442cfc57690
      e442cfc57690
  3750ebee865d
      392fd25390da
  7ae126973a96
      a139f71be9da
  14608b260df8
      a139f71be9da
  bed64f5d2f5a
      a139f71be9da
  a139f71be9da
      a139f71be9da
  $ hg log -r 'contentdivergent()'

  $ cd ..


Subset does not diverge
------------------------------

Do not report divergent successors-set if it is a subset of another
successors-set. (report [A,B] not [A] + [A,B])

  $ newcase subset
  $ hg debugobsolete `getid A_0` `getid A_2`
  obsoleted 1 changesets
  $ hg debugobsolete `getid A_0` `getid A_1` `getid A_2`
  $ hg debugsuccessorssets --hidden 'desc('A_0')'
  007dc284c1f8
      82623d38b9ba 392fd25390da
  $ hg debugsuccessorssets 'desc('A_0')' --closest
  $ hg debugsuccessorssets 'desc('A_0')' --closest --hidden
  007dc284c1f8
      82623d38b9ba 392fd25390da

  $ cd ..

Use scmutil.cleanupnodes API to create divergence

  $ hg init cleanupnodes
  $ cd cleanupnodes
  $ hg debugdrawdag <<'EOS'
  >   B1  B3 B4
  >   |     \|
  >   A      Z
  > EOS

  $ hg update -q B1
  $ echo 3 >> B
  $ hg commit --amend -m B2
  $ cat > $TESTTMP/scmutilcleanup.py <<EOF
  > from edenscm.mercurial import registrar, scmutil
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('cleanup')
  > def cleanup(ui, repo):
  >     def node(expr):
  >         unfi = repo.unfiltered()
  >         rev = unfi.revs(expr).first()
  >         return unfi.changelog.node(rev)
  >     with repo.wlock(), repo.lock(), repo.transaction('delayedstrip'):
  >         mapping = {node('desc(B1)'): [node('desc(B3)')],
  >                    node('desc(B3)'): [node('desc(B4)')]}
  >         scmutil.cleanupnodes(repo, mapping, 'test')
  > EOF

  $ rm .hg/localtags
  $ hg cleanup --config extensions.t=$TESTTMP/scmutilcleanup.py
  $ hg log -G -T '{rev}:{node|short} {desc} {instabilities}' -r 'sort(all(), topo)'
  @  5:1a2a9b5b0030 B2 content-divergent
  |
  | o  4:70d5a63ca112 B4 content-divergent
  | |
  | o  1:48b9aae0607f Z
  |
  o  0:426bada5c675 A
  
  $ hg debugobsolete
  a178212c3433c4e77b573f6011e29affb8aefa33 1a2a9b5b0030632400aa78e00388c20f99d3ec44 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'}
  a178212c3433c4e77b573f6011e29affb8aefa33 ad6478fb94ecec98b86daae98722865d494ac561 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'test', 'user': 'test'}
  ad6478fb94ecec98b86daae98722865d494ac561 70d5a63ca112acb3764bc1d7320ca90ea688d671 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'test', 'user': 'test'}
