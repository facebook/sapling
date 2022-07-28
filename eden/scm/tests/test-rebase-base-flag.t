#chg-compatible

Test the "--base" flag of the rebase command. (Tests unrelated to the "--base"
flag should probably live in somewhere else)

  $ enable rebase
  $ setconfig phases.publish=false

  $ rebasewithdag() {
  >   N=$((N+1))
  >   hg init repo$N && cd repo$N
  >   hg debugdrawdag
  >   hg rebase "$@" > _rebasetmp
  >   r=$?
  >   grep -v 'saved backup bundle' _rebasetmp
  >   hg book -d `hg book -T '{bookmark} '`
  >   [ $r -eq 0 ] && tglog
  >   cd ..
  >   return $r
  > }

Single branching point, without merge:

  $ rebasewithdag -b D -d Z <<'EOS'
  >     D E
  >     |/
  > Z B C   # C: branching point, E should be picked
  >  \|/    # B should not be picked
  >   A
  >   |
  >   R
  > EOS
  rebasing d6003a550c2c "C" (C)
  rebasing 4526cf523425 "D" (D)
  rebasing b296604d9846 "E" (E)
  o  4870f5e7df37 'E'
  │
  │ o  dc999528138a 'D'
  ├─╯
  o  6b3e11729672 'C'
  │
  o  57e70bad1ea3 'Z'
  │
  │ o  c1e6b162678d 'B'
  ├─╯
  o  21a6c4502885 'A'
  │
  o  b41ce7760717 'R'
  
Multiple branching points caused by selecting a single merge changeset:

  $ rebasewithdag -b E -d Z <<'EOS'
  >     E
  >    /|
  >   B C D  # B, C: multiple branching points
  >   | |/   # D should not be picked
  > Z | /
  >  \|/
  >   A
  >   |
  >   R
  > EOS
  rebasing c1e6b162678d "B" (B)
  rebasing d6003a550c2c "C" (C)
  rebasing 54c8f00cb91c "E" (E)
  o    00598421b616 'E'
  ├─╮
  │ o  6b3e11729672 'C'
  │ │
  o │  85260910e847 'B'
  ├─╯
  o  57e70bad1ea3 'Z'
  │
  │ o  8924700906fe 'D'
  ├─╯
  o  21a6c4502885 'A'
  │
  o  b41ce7760717 'R'
  
Rebase should not extend the "--base" revset using "descendants":

  $ rebasewithdag -b B -d Z <<'EOS'
  >     E
  >    /|
  > Z B C  # descendants(B) = B+E. With E, C will be included incorrectly
  >  \|/
  >   A
  >   |
  >   R
  > EOS
  rebasing c1e6b162678d "B" (B)
  rebasing 54c8f00cb91c "E" (E)
  o    e583bf3ff54c 'E'
  ├─╮
  │ o  85260910e847 'B'
  │ │
  │ o  57e70bad1ea3 'Z'
  │ │
  o │  d6003a550c2c 'C'
  ├─╯
  o  21a6c4502885 'A'
  │
  o  b41ce7760717 'R'
  
Rebase should not simplify the "--base" revset using "roots":

  $ rebasewithdag -b B+E -d Z <<'EOS'
  >     E
  >    /|
  > Z B C  # roots(B+E) = B. Without E, C will be missed incorrectly
  >  \|/
  >   A
  >   |
  >   R
  > EOS
  rebasing c1e6b162678d "B" (B)
  rebasing d6003a550c2c "C" (C)
  rebasing 54c8f00cb91c "E" (E)
  o    00598421b616 'E'
  ├─╮
  │ o  6b3e11729672 'C'
  │ │
  o │  85260910e847 'B'
  ├─╯
  o  57e70bad1ea3 'Z'
  │
  o  21a6c4502885 'A'
  │
  o  b41ce7760717 'R'
  
The destination is one of the two branching points of a merge:

  $ rebasewithdag -b F -d Z <<'EOS'
  >     F
  >    / \
  >   E   D
  >  /   /
  > Z   C
  >  \ /
  >   B
  >   |
  >   A
  > EOS
  nothing to rebase
  o    e7414f308889 'F'
  ├─╮
  │ o  64aa4c30955e 'E'
  │ │
  o │  f585351a92f8 'D'
  │ │
  │ o  4f40d47d3d68 'Z'
  │ │
  o │  26805aba1e60 'C'
  ├─╯
  o  112478962961 'B'
  │
  o  426bada5c675 'A'
  

Multiple branching points caused by multiple bases (issue5420):

  $ rebasewithdag -b E1+E2+C2+B1 -d Z <<'EOS'
  >   Z    E2
  >   |   /
  >   F E1 C2
  >   |/  /
  >   E C1 B2
  >   |/  /
  >   C B1
  >   |/
  >   B
  >   |
  >   A
  >   |
  >   R
  > EOS
  rebasing a113dbaa660a "B1" (B1)
  rebasing 06ce7b1cc8c2 "B2" (B2)
  rebasing 0ac98cce32d3 "C1" (C1)
  rebasing 781512f5e33d "C2" (C2)
  rebasing 428d8c18f641 "E1" (E1)
  rebasing e1bf82f6b6df "E2" (E2)
  o  e4a37b6fdbd2 'E2'
  │
  o  9675bea983df 'E1'
  │
  │ o  4faf5d4c80dc 'C2'
  │ │
  │ o  d4799b1ad57d 'C1'
  ├─╯
  │ o  772732dc64d6 'B2'
  │ │
  │ o  ad3ac528a49f 'B1'
  ├─╯
  o  2cbdfca6b9d5 'Z'
  │
  o  fcdb3293ec13 'F'
  │
  o  a4652bb8ac54 'E'
  │
  o  bd5548558fcf 'C'
  │
  o  c1e6b162678d 'B'
  │
  o  21a6c4502885 'A'
  │
  o  b41ce7760717 'R'
  
Multiple branching points with multiple merges:

  $ rebasewithdag -b G+P -d Z <<'EOS'
  > G   H   P
  > |\ /|   |\
  > F E D   M N
  >  \|/|  /| |\
  > Z C B I J K L
  >  \|/  |/  |/
  >   A   A   A
  > EOS
  rebasing dc0947a82db8 "C" (C)
  rebasing afc707c82df0 "F" (F)
  rebasing 4e4f9194f9f1 "D" (D)
  rebasing 03ca77807e91 "E" (E)
  rebasing 690dfff91e9e "G" (G)
  rebasing 2893b886bb10 "H" (H)
  rebasing 08ebfeb61bac "I" (I)
  rebasing a0a5005cec67 "J" (J)
  rebasing d1f6d0c3c7e4 "M" (M)
  rebasing e131637a1cb6 "L" (L)
  rebasing 83780307a7e8 "K" (K)
  rebasing 7aaec6f81888 "N" (N)
  rebasing 325bc8f1760d "P" (P)
  o    6ef6a0ea3b18 'P'
  ├─╮
  │ o    20ba3610a7e5 'N'
  │ ├─╮
  │ │ o  7bbb6c8a6ad7 'K'
  │ │ │
  │ o │  bca872041455 'L'
  │ ├─╯
  o │    cd4f6c06d2ab 'M'
  ├───╮
  │ │ o  de0cbffe893e 'J'
  │ ├─╯
  o │  0e710f176a88 'I'
  ├─╯
  │ o    52507bab39ca 'H'
  │ ├─╮
  │ │ │ o  bb5fe4652f0d 'G'
  │ │ ╭─┤
  │ │ o │  b168f85f2e78 'E'
  │ │ │ │
  │ o │ │  8d09fcdb5594 'D'
  │ ├─╮ │
  │ │ │ o  f4ad4b31daf4 'F'
  │ │ ├─╯
  │ │ o  ab70b4c5a9c9 'C'
  ├───╯
  o │  262e37e34f63 'Z'
  │ │
  │ o  112478962961 'B'
  ├─╯
  o  426bada5c675 'A'
  
Slightly more complex merge case (mentioned in https://www.mercurial-scm.org/pipermail/mercurial-devel/2016-November/091074.html):

  $ rebasewithdag -b A3+B3 -d Z <<'EOF'
  > Z     C1    A3     B3
  > |    /     / \    / \
  > M3 C0     A1  A2 B1  B2
  > | /       |   |  |   |
  > M2        M1  C1 C1  M3
  > |
  > M1
  > |
  > M0
  > EOF
  rebasing 8817fae53c94 "C0" (C0)
  rebasing 73508237b032 "C1" (C1)
  rebasing fdb955e2faed "A2" (A2)
  rebasing 4e449bd1a643 "A3" (A3)
  rebasing 0a33b0519128 "B1" (B1)
  rebasing 06ca5dfe3b5b "B2" (B2)
  rebasing 209327807c3a "B3" (B3)
  o    ceb984566332 'B3'
  ├─╮
  │ o  74275896650e 'B2'
  │ │
  o │  19d93caac497 'B1'
  │ │
  │ │ o    058e73d3916b 'A3'
  │ │ ├─╮
  │ │ │ o  0ba13ad72234 'A2'
  ├─────╯
  o │ │  c122c2af10c6 'C1'
  │ │ │
  o │ │  455ba9bd3ea2 'C0'
  ├─╯ │
  o   │  b3d7d2fda53b 'Z'
  │   │
  o   │  182ab6383dd7 'M3'
  │   │
  o   │  6c3f73563d5f 'M2'
  │   │
  │   o  88c860fffcc2 'A1'
  ├───╯
  o  bc852baa85dd 'M1'
  │
  o  dbdfc5c9bcd5 'M0'
  
Disconnected graph:

  $ rebasewithdag -b B -d Z <<'EOS'
  >   B
  >   |
  > Z A
  > EOS
  nothing to rebase from 112478962961 to 48b9aae0607f
  o  112478962961 'B'
  │
  │ o  48b9aae0607f 'Z'
  │
  o  426bada5c675 'A'
  

Multiple roots. Roots are ancestors of dest:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > D Z B
  >  \|\|
  >   C A
  > EOF
  rebasing 112478962961 "B" (B)
  rebasing b70f76719894 "D" (D)
  o  511efad7bf13 'D'
  │
  │ o  25c4e279af62 'B'
  ├─╯
  o    3a49f54d7bb1 'Z'
  ├─╮
  │ o  96cc3511f894 'C'
  │
  o  426bada5c675 'A'
  
Multiple roots. One root is not an ancestor of dest:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > Z B D
  >  \|\|
  >   A C
  > EOF
  nothing to rebase from f675d5a1c6a4+b70f76719894 to 262e37e34f63
  o  262e37e34f63 'Z'
  │
  │ o  b70f76719894 'D'
  │ │
  │ │ o  f675d5a1c6a4 'B'
  ╭─┬─╯
  │ o  96cc3511f894 'C'
  │
  o  426bada5c675 'A'
  

Multiple roots. One root is not an ancestor of dest. Select using a merge:

  $ rebasewithdag -b E -d Z <<'EOF'
  >   E
  >   |\
  > Z B D
  >  \|\|
  >   A C
  > EOF
  rebasing f675d5a1c6a4 "B" (B)
  rebasing f68696fe6af8 "E" (E)
  o    f6e6f5081554 'E'
  ├─╮
  │ o    30cabcba27be 'B'
  │ ├─╮
  │ │ o  262e37e34f63 'Z'
  │ │ │
  o │ │  b70f76719894 'D'
  ├─╯ │
  o   │  96cc3511f894 'C'
      │
      o  426bada5c675 'A'
  
Multiple roots. Two children share two parents while dest has only one parent:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > Z B D
  >  \|\|\
  >   A C A
  > EOF
  rebasing f675d5a1c6a4 "B" (B)
  rebasing c2a779e13b56 "D" (D)
  o    5eecd056b5f8 'D'
  ├─╮
  │ │ o  30cabcba27be 'B'
  ╭─┬─╯
  │ o  262e37e34f63 'Z'
  │ │
  o │  96cc3511f894 'C'
    │
    o  426bada5c675 'A'
  
