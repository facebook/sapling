#chg-compatible

  $ configure evolution

  $ hg init inhibit
  $ cd inhibit

  $ hg debugdrawdag <<'EOS'
  > B1 B2   # amend: B1 -> B2
  >  |/
  >  A
  > EOS

  $ hg up null -q
  $ B1=`HGPLAIN=1 hg log -r B1 -T '{node}' --hidden`
  $ B2=`HGPLAIN=1 hg log -r B2 -T '{node}' --hidden`

  $ hg debugobsolete $B2 $B1 -d '1 0'
  obsoleted 1 changesets
  $ hg log -G -T '{desc}' --hidden
  x  B2
  |
  | o  B1
  |/
  o  A
  
  $ hg debugobsolete $B1 $B2 -d '2 0'
  $ hg log -G -T '{desc}' --hidden
  o  B2
  |
  | x  B1
  |/
  o  A
  
  $ hg debugobsolete $B1 $B1 -d '3 0'
  $ hg log -G -T '{desc}' --hidden
  o  B2
  |
  | o  B1
  |/
  o  A
  
Test revive works inside a transaction

  $ cat > $TESTTMP/revivetest.py <<'EOF'
  > from __future__ import absolute_import, print_function
  > from edenscm.mercurial import extensions, obsolete, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('revivetest')
  > def revivetest(ui, repo, revset):
  >     with repo.wlock(), repo.lock(), repo.transaction('revivetest') as tr:
  >         ctxs = list(repo.unfiltered().set(revset))
  >         obsolete.revive(ctxs)
  >         # make sure they are revived by checking them
  >         for ctx in ctxs:
  >             if not repo[ctx.node()].obsolete() and not ctx.obsolete():
  >                 ui.write('%s is revived\n' % ctx.description())
  >             else:
  >                 ui.write('%s is NOT revived\n' % ctx.description())
  > EOF

  $ hg init $TESTTMP/revivetest
  $ cd $TESTTMP/revivetest
  $ hg debugdrawdag <<'EOS'
  > C E F  # split: B -> D, E
  > | |/   # amend: E -> F
  > B D    # prune: F, D, A
  > |/
  > A
  > EOS
  $ hg log -G -T '{rev} {desc} {node|short}' --hidden
  x  5 F ad6717a6a58e
  |
  | x  4 E 4b61ff5c62e2
  |/
  | o  3 C 26805aba1e60
  | |
  x |  2 D b18e25de2cf5
  | |
  | x  1 B 112478962961
  |/
  x  0 A 426bada5c675
  
  $ hg debugobsolete
  112478962961147124edd43549aedd1a335e44bf b18e25de2cf5fc4699a029ed635882849e53ef73 4b61ff5c62e28cff36152201967390a6e7375604 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'split', 'user': 'test'}
  4b61ff5c62e28cff36152201967390a6e7375604 ad6717a6a58e34efe818b17b745fb04e0d17d49e 0 (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'amend', 'user': 'test'}
  ad6717a6a58e34efe818b17b745fb04e0d17d49e 0 {b18e25de2cf5fc4699a029ed635882849e53ef73} (Thu Jan 01 00:00:02 1970 +0000) {'operation': 'prune', 'user': 'test'}
  b18e25de2cf5fc4699a029ed635882849e53ef73 0 {426bada5c67598ca65036d57d9e4b64b0c1ce7a0} (Thu Jan 01 00:00:01 1970 +0000) {'operation': 'prune', 'user': 'test'}
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 0 {0000000000000000000000000000000000000000} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'prune', 'user': 'test'}
  $ hg revivetest 'obsolete()' --config extensions.revivetest=$TESTTMP/revivetest.py
  A is revived
  B is revived
  D is revived
  E is revived
  F is revived

  $ hg log -G -T '{rev} {desc} {node|short}'
  o  5 F ad6717a6a58e
  |
  | o  4 E 4b61ff5c62e2
  |/
  | o  3 C 26805aba1e60
  | |
  o |  2 D b18e25de2cf5
  | |
  | o  1 B 112478962961
  |/
  o  0 A 426bada5c675
  
Test date is set correctly

  $ hg debugdrawdag << 'EOS'
  > G
  > |
  > C
  > EOS

  $ hg update -q G
  $ echo 1 >> G
  $ hg commit --amend -m G1 --config devel.default-date='123456 0'
  $ hg unamend --config extensions.amend=
  $ hg debugobsolete | tail -1
  b7e6ea8355346dbbedcb23188c3a5de400e780ae cf43ad9da869111bee585c5475aefe61f6f74649 0 (Fri Jan 02 10:17:37 1970 +0000) {'user': 'test'}
  $ echo 2 >> G

Do not use a mocked date

  $ cat >> .hg/hgrc <<EOF
  > [devel]
  > %unset default-date
  > EOF
  $ hg commit --amend -m G2
  $ hg unamend --config extensions.amend=
  $ hg debugobsolete | tail -1
  51313a3d2b65bd2cbc4828a7309af84eb5914bdb cf43ad9da869111bee585c5475aefe61f6f74649 0 (*) {'user': 'test'} (glob)
  $ hg debugobsolete | tail -1 | grep ' 1970 +0000'
  [1]
