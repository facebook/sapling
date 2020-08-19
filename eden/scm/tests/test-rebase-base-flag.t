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
  o  9: 4870f5e7df37 'E'
  |
  | o  8: dc999528138a 'D'
  |/
  o  7: 6b3e11729672 'C'
  |
  o  4: 57e70bad1ea3 'Z'
  |
  | o  2: c1e6b162678d 'B'
  |/
  o  1: 21a6c4502885 'A'
  |
  o  0: b41ce7760717 'R'
  
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
  o    9: 00598421b616 'E'
  |\
  | o  8: 6b3e11729672 'C'
  | |
  o |  7: 85260910e847 'B'
  |/
  o  5: 57e70bad1ea3 'Z'
  |
  | o  4: 8924700906fe 'D'
  |/
  o  1: 21a6c4502885 'A'
  |
  o  0: b41ce7760717 'R'
  
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
  o    7: e583bf3ff54c 'E'
  |\
  | o  6: 85260910e847 'B'
  | |
  | o  4: 57e70bad1ea3 'Z'
  | |
  o |  3: d6003a550c2c 'C'
  |/
  o  1: 21a6c4502885 'A'
  |
  o  0: b41ce7760717 'R'
  
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
  o    8: 00598421b616 'E'
  |\
  | o  7: 6b3e11729672 'C'
  | |
  o |  6: 85260910e847 'B'
  |/
  o  4: 57e70bad1ea3 'Z'
  |
  o  1: 21a6c4502885 'A'
  |
  o  0: b41ce7760717 'R'
  
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
  o    6: e7414f308889 'F'
  |\
  | o  5: 64aa4c30955e 'E'
  | |
  o |  4: f585351a92f8 'D'
  | |
  | o  3: 4f40d47d3d68 'Z'
  | |
  o |  2: 26805aba1e60 'C'
  |/
  o  1: 112478962961 'B'
  |
  o  0: 426bada5c675 'A'
  

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
  o  18: e4a37b6fdbd2 'E2'
  |
  o  17: 9675bea983df 'E1'
  |
  | o  16: 4faf5d4c80dc 'C2'
  | |
  | o  15: d4799b1ad57d 'C1'
  |/
  | o  14: 772732dc64d6 'B2'
  | |
  | o  13: ad3ac528a49f 'B1'
  |/
  o  12: 2cbdfca6b9d5 'Z'
  |
  o  10: fcdb3293ec13 'F'
  |
  o  7: a4652bb8ac54 'E'
  |
  o  4: bd5548558fcf 'C'
  |
  o  2: c1e6b162678d 'B'
  |
  o  1: 21a6c4502885 'A'
  |
  o  0: b41ce7760717 'R'
  
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
  rebasing 03ca77807e91 "E" (E)
  rebasing afc707c82df0 "F" (F)
  rebasing 690dfff91e9e "G" (G)
  rebasing 4e4f9194f9f1 "D" (D)
  rebasing 2893b886bb10 "H" (H)
  rebasing 83780307a7e8 "K" (K)
  rebasing e131637a1cb6 "L" (L)
  rebasing 7aaec6f81888 "N" (N)
  rebasing 08ebfeb61bac "I" (I)
  rebasing a0a5005cec67 "J" (J)
  rebasing d1f6d0c3c7e4 "M" (M)
  rebasing 325bc8f1760d "P" (P)
  o    28: 6ef6a0ea3b18 'P'
  |\
  | o    27: cd4f6c06d2ab 'M'
  | |\
  | | o  26: de0cbffe893e 'J'
  | | |
  | o |  25: 0e710f176a88 'I'
  | |/
  o |    24: 20ba3610a7e5 'N'
  |\ \
  | o |  23: bca872041455 'L'
  | |/
  o /  22: 7bbb6c8a6ad7 'K'
  |/
  | o    21: 52507bab39ca 'H'
  | |\
  | | o    20: 8d09fcdb5594 'D'
  | | |\
  | +-----o  19: bb5fe4652f0d 'G'
  | | | | |
  | | | | o  18: f4ad4b31daf4 'F'
  | | | |/
  | o---+  17: b168f85f2e78 'E'
  |  / /
  +---o  16: ab70b4c5a9c9 'C'
  | |
  o |  7: 262e37e34f63 'Z'
  | |
  | o  1: 112478962961 'B'
  |/
  o  0: 426bada5c675 'A'
  
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
  rebasing 06ca5dfe3b5b "B2" (B2)
  rebasing 0a33b0519128 "B1" (B1)
  rebasing 209327807c3a "B3" (B3)
  o    19: ceb984566332 'B3'
  |\
  | o  18: 19d93caac497 'B1'
  | |
  o |  17: 74275896650e 'B2'
  | |
  | | o    16: 058e73d3916b 'A3'
  | | |\
  | +---o  15: 0ba13ad72234 'A2'
  | | |
  | o |  14: c122c2af10c6 'C1'
  | | |
  | o |  13: 455ba9bd3ea2 'C0'
  |/ /
  o |  8: b3d7d2fda53b 'Z'
  | |
  o |  5: 182ab6383dd7 'M3'
  | |
  o |  3: 6c3f73563d5f 'M2'
  | |
  | o  2: 88c860fffcc2 'A1'
  |/
  o  1: bc852baa85dd 'M1'
  |
  o  0: dbdfc5c9bcd5 'M0'
  
Disconnected graph:

  $ rebasewithdag -b B -d Z <<'EOS'
  >   B
  >   |
  > Z A
  > EOS
  nothing to rebase from 112478962961 to 48b9aae0607f
  o  2: 112478962961 'B'
  |
  | o  1: 48b9aae0607f 'Z'
  |
  o  0: 426bada5c675 'A'
  

Multiple roots. Roots are ancestors of dest:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > D Z B
  >  \|\|
  >   C A
  > EOF
  rebasing 112478962961 "B" (B)
  rebasing b70f76719894 "D" (D)
  o  6: 511efad7bf13 'D'
  |
  | o  5: 25c4e279af62 'B'
  |/
  o    4: 3a49f54d7bb1 'Z'
  |\
  | o  1: 96cc3511f894 'C'
  |
  o  0: 426bada5c675 'A'
  
Multiple roots. One root is not an ancestor of dest:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > Z B D
  >  \|\|
  >   A C
  > EOF
  nothing to rebase from f675d5a1c6a4+b70f76719894 to 262e37e34f63
  o  4: 262e37e34f63 'Z'
  |
  | o  3: b70f76719894 'D'
  | |
  +---o  2: f675d5a1c6a4 'B'
  | |/
  | o  1: 96cc3511f894 'C'
  |
  o  0: 426bada5c675 'A'
  

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
  o    7: f6e6f5081554 'E'
  |\
  | o    6: 30cabcba27be 'B'
  | |\
  | | o  4: 262e37e34f63 'Z'
  | | |
  o | |  3: b70f76719894 'D'
  |/ /
  o /  1: 96cc3511f894 'C'
   /
  o  0: 426bada5c675 'A'
  
Multiple roots. Two children share two parents while dest has only one parent:

  $ rebasewithdag -b B+D -d Z <<'EOF'
  > Z B D
  >  \|\|\
  >   A C A
  > EOF
  rebasing f675d5a1c6a4 "B" (B)
  rebasing c2a779e13b56 "D" (D)
  o    6: 5eecd056b5f8 'D'
  |\
  +---o  5: 30cabcba27be 'B'
  | |/
  | o  4: 262e37e34f63 'Z'
  | |
  o |  1: 96cc3511f894 'C'
   /
  o  0: 426bada5c675 'A'
  
