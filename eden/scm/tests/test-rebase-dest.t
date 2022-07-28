#chg-compatible
  $ setconfig experimental.allowfilepeer=True

Require a destination
  $ enable rebase
  $ setconfig commands.rebase.requiredest=true
  $ hg init repo
  $ cd repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ echo b >> b
  $ hg commit -qAm bb
  $ hg up ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg commit -qAm cc
  $ hg rebase
  abort: you must specify a destination
  (use: hg rebase -d REV)
  [255]
  $ hg rebase -d 'desc(bb)'
  rebasing 5db65b93a12b "cc"
  $ hg rebase -d 'desc(aa)' -r . -q
  $ HGPLAIN=1 hg rebase
  rebasing 889b0bc6a730 "cc"
  $ hg rebase -d 'desc(aa)' -r . -q
  $ hg --config commands.rebase.requiredest=False rebase
  rebasing 279de9495438 "cc"

Requiring dest should not break continue or other rebase options
  $ hg up 'desc(bb)' -q
  $ echo d >> c
  $ hg commit -qAm dc
  $ hg log -G -T '{desc}'
  @  dc
  │
  │ o  cc
  ├─╯
  o  bb
  │
  o  aa
  
  $ hg rebase -d 'desc(cc)'
  rebasing 0537f6b50def "dc"
  merging c
  warning: 1 conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo d > c
  $ hg resolve --mark --all
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 0537f6b50def "dc"

  $ cd ..

Check rebase.requiredest interaction with pull --rebase
  $ hg clone repo clone
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo
  $ echo e > e
  $ hg commit -qAm ee
  $ cd ..
  $ cd clone
  $ echo f > f
  $ hg commit -qAm ff
  $ hg pull --rebase
  abort: rebase destination required by configuration
  (use hg pull followed by hg rebase -d DEST)
  [255]

Setup rebase with multiple destinations

  $ cd $TESTTMP

  $ cat >> $TESTTMP/maprevset.py <<EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import registrar, revset, revsetlang, smartset
  > revsetpredicate = registrar.revsetpredicate()
  > cache = {}
  > @revsetpredicate('map')
  > def map(repo, subset, x):
  >     """(set, mapping)"""
  >     setarg, maparg = revsetlang.getargs(x, 2, 2, '')
  >     rset = revset.getset(repo, smartset.fullreposet(repo), setarg)
  >     mapstr = revsetlang.getstring(maparg, '')
  >     map = dict(a.split(':') for a in mapstr.split(','))
  >     rev = rset.first()
  >     desc = repo[rev].description()
  >     newdesc = map.get(desc)
  >     if newdesc == 'null':
  >         revs = [-1]
  >     else:
  >         query = revsetlang.formatspec('desc(%s)', newdesc)
  >         revs = repo.revs(query)
  >     return smartset.baseset(revs, repo=repo)
  > EOF

  $ setglobalconfig ui.allowemptycommit=1 phases.publish=false
  $ setglobalconfig experimental.evolution=true
  $ setglobalconfig extensions.maprevset="$TESTTMP/maprevset.py"
  $ readglobalconfig <<EOF
  > [alias]
  > tglog = log -G --template "{node|short} {desc} {instabilities}" -r 'sort(all(), topo)'
  > EOF

  $ rebasewithdag() {
  >   N=$((N + 1))
  >   hg init repo$N && cd repo$N
  >   hg debugdrawdag
  >   hg rebase "$@" > _rebasetmp
  >   r=$?
  >   grep -v 'saved backup bundle' _rebasetmp
  >   [ $r -eq 0 ] && rm -f .hg/localtags && hg book -d `hg book -T '{bookmark} '` && tglog
  >   cd ..
  >   return $r
  > }

Destination resolves to an empty set:

  $ rebasewithdag -s B -d 'SRC - SRC' <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  nothing to rebase - empty destination
  o  26805aba1e60 'C'
  │
  o  112478962961 'B'
  │
  o  426bada5c675 'A'
  

Multiple destinations and --collapse are not compatible:

  $ rebasewithdag -s C+E -d 'SRC^^' --collapse <<'EOS'
  > C F
  > | |
  > B E
  > | |
  > A D
  > EOS
  abort: --collapse does not work with multiple destinations
  [255]

Multiple destinations cannot be used with --base:

  $ rebasewithdag -b B+E -d 'SRC^^' --collapse <<'EOS'
  > B E
  > | |
  > A D
  > EOS
  abort: unknown revision 'SRC'!
  [255]

Rebase to null should work:

  $ rebasewithdag -r A+C+D -d 'null' <<'EOS'
  > C D
  > | |
  > A B
  > EOS
  already rebased 426bada5c675 "A" (A)
  already rebased dc0947a82db8 "C" (C)
  rebasing 004dc1679908 "D" (D)
  o  d8d8601abd5e 'D'
  
  o  dc0947a82db8 'C'
  │
  │ o  fc2b737bb2e5 'B'
  │
  o  426bada5c675 'A'
  
Destination resolves to multiple changesets:

  $ rebasewithdag -s B -d 'ALLSRC+SRC' <<'EOS'
  > C
  > |
  > B
  > |
  > Z
  > EOS
  abort: rebase destination for f0a671a46792 is not unique
  [255]

Destination is an ancestor of source:

  $ rebasewithdag -s B -d 'SRC' <<'EOS'
  > C
  > |
  > B
  > |
  > Z
  > EOS
  abort: source and destination form a cycle
  [255]

Switch roots:

  $ rebasewithdag -s 'all() - roots(all())' -d 'roots(all()) - ::SRC' <<'EOS'
  > C  F
  > |  |
  > B  E
  > |  |
  > A  D
  > EOS
  rebasing 112478962961 "B" (B)
  rebasing 26805aba1e60 "C" (C)
  rebasing cd488e83d208 "E" (E)
  rebasing 0069ba24938a "F" (F)
  o  d150ff263fc8 'F'
  │
  o  66f30a1a2eab 'E'
  │
  │ o  93db94ffae0e 'C'
  │ │
  │ o  d0071c3b0c88 'B'
  │ │
  │ o  058c1e1fb10a 'D'
  │
  o  426bada5c675 'A'
  
Different destinations for merge changesets with a same root:

  $ rebasewithdag -s B -d '((parents(SRC)-B-A)::) - (::ALLSRC)' <<'EOS'
  > C G
  > |\|
  > | F
  > |
  > B E
  > |\|
  > A D
  > EOS
  rebasing a4256619d830 "B" (B)
  rebasing 8e139e245220 "C" (C)
  o    51e2ce92e06a 'C'
  ├─╮
  │ o    2ed0c8546285 'B'
  │ ├─╮
  o │ │  8fdb2c1feb20 'G'
  │ │ │
  │ │ o  cd488e83d208 'E'
  │ │ │
  o │ │  a6661b868de9 'F'
    │ │
    │ o  058c1e1fb10a 'D'
    │
    o  426bada5c675 'A'
  
Move to a previous parent:

  $ rebasewithdag -s E+F+G -d 'SRC^^' <<'EOS'
  >     H
  >     |
  >   D G
  >   |/
  >   C F
  >   |/
  >   B E  # E will be ignored, since E^^ is empty
  >   |/
  >   A
  > EOS
  rebasing cf43ad9da869 "G" (G)
  rebasing eef94f3b5f03 "H" (H)
  rebasing 33441538d4aa "F" (F)
  o  02aa697facf7 'F'
  │
  │ o  b3d84c6666cf 'H'
  │ │
  │ │ o  f7c28a1a15e2 'G'
  │ │ │
  │ │ │ o  f585351a92f8 'D'
  │ ├───╯
  │ o │  26805aba1e60 'C'
  │ ├─╯
  │ │ o  7fb047a69f22 'E'
  ├───╯
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  
Source overlaps with destination:

  $ rebasewithdag -s 'B+C+D' -d 'map(SRC, "B:C,C:D")' <<'EOS'
  > B C D
  >  \|/
  >   A
  > EOS
  rebasing dc0947a82db8 "C" (C)
  rebasing 112478962961 "B" (B)
  o  5fe9935d5222 'B'
  │
  o  12d20731b9e0 'C'
  │
  o  b18e25de2cf5 'D'
  │
  o  426bada5c675 'A'
  
Detect cycles early:

  $ rebasewithdag -r 'all()-Z' -d 'map(SRC, "A:B,B:C,C:D,D:B")' <<'EOS'
  > A B C
  >  \|/
  >   | D
  >   |/
  >   Z
  > EOS
  abort: source and destination form a cycle
  [255]

Detect source is ancestor of dest in runtime:

  $ rebasewithdag -r 'C+B' -d 'map(SRC, "C:B,B:D")' -q <<'EOS'
  >   D
  >   |
  > B C
  >  \|
  >   A
  > EOS
  abort: source is ancestor of destination
  [255]

"Already rebased" fast path still works:

  $ rebasewithdag -r 'all()' -d 'SRC^' <<'EOS'
  >   E F
  >  /| |
  > B C D
  >  \|/
  >   A
  > EOS
  already rebased 112478962961 "B" (B)
  already rebased dc0947a82db8 "C" (C)
  already rebased b18e25de2cf5 "D" (D)
  already rebased 312782b8f06e "E" (E)
  already rebased ad6717a6a58e "F" (F)
  o  ad6717a6a58e 'F'
  │
  │ o    312782b8f06e 'E'
  │ ├─╮
  o │ │  b18e25de2cf5 'D'
  │ │ │
  │ │ o  dc0947a82db8 'C'
  ├───╯
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  
Massively rewrite the DAG:

  $ rebasewithdag -r 'all()' -d 'map(SRC, "A:I,I:null,H:A,B:J,J:C,C:H,D:E,F:G,G:K,K:D,E:B")' <<'EOS'
  > D G K
  > | | |
  > C F J
  > | | |
  > B E I
  >  \| |
  >   A H
  > EOS
  rebasing 701514e1408d "I" (I)
  rebasing 426bada5c675 "A" (A)
  rebasing e7050b6e5048 "H" (H)
  rebasing 26805aba1e60 "C" (C)
  rebasing cf89f86b485b "J" (J)
  rebasing 112478962961 "B" (B)
  rebasing 7fb047a69f22 "E" (E)
  rebasing f585351a92f8 "D" (D)
  rebasing ae41898d7875 "K" (K)
  rebasing 711f53bbef0b "G" (G)
  rebasing 64a8289d2492 "F" (F)
  o  3735afb3713a 'F'
  │
  o  07698142d7a7 'G'
  │
  o  33aba52e7e72 'K'
  │
  o  9fdae89dc5a1 'D'
  │
  o  277dda9a65ee 'E'
  │
  o  9c74fd8657ad 'B'
  │
  o  6527eb0688bb 'J'
  │
  o  e94d655b928d 'C'
  │
  o  620d6d349459 'H'
  │
  o  a569a116758f 'A'
  │
  o  2bf1302f5c18 'I'
  
Resolve instability:

  $ rebasewithdag <<'EOF' -r '(obsolete()::)-obsolete()' -d 'max((successors(max(roots(ALLSRC) & ::SRC)^)-obsolete())::)'
  >      F2
  >      |
  >    J E E2
  >    | |/
  > I2 I | E3
  >   \| |/
  >    H | G
  >    | | |
  >   B2 D F
  >    | |/         # rebase: B -> B2
  >    N C          # amend: E -> E2
  >    | |          # amend: E2 -> E3
  >    M B          # rebase: F -> F2
  >     \|          # amend: I -> I2
  >      A
  > EOF
  rebasing 5c432343bf59 "J" (J)
  rebasing 26805aba1e60 "C" (C)
  rebasing f585351a92f8 "D" (D)
  rebasing ffebc37c5d0b "E3" (E3)
  rebasing fb184bcfeee8 "F2" (F2)
  rebasing dc838ab4c0da "G" (G)
  o  174f63d574a8 'G'
  │
  o  c9d9fbe76705 'F2'
  │
  o  0a03c2ede755 'E3'
  │
  o  228d9d2541b1 'D'
  │
  o  cd856b400c95 'C'
  │
  o  9148200c858c 'J'
  │
  o  eb74780f5094 'I2'
  │
  o  78309edd643f 'H'
  │
  o  4b4531bd8e1d 'B2'
  │
  o  337c285c272b 'N'
  │
  o  699bc4b6fa22 'M'
  │
  o  426bada5c675 'A'
  
