  $ configure modern
  $ enable rebaseconflict

  $ newremoterepo
  $ drawdag << 'EOS'
  >         # D/A=A
  > B C D   # C/A=C
  >  \|/    # B/A=B
  >   A     # A/A=A
  > EOS

Rebase to C, cause conflict

  $ hg debugrebaseconflict -r $B -d $C
  $ hg debugshowconflict -r 'max(desc(B))'
  0825cd035008: 1 conflicts
    A: adds=7145412529cd,48fb0d73963d removes=426bada5c675
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  │ o  D
  │ │
  o │  C
  ├─╯
  o  A
  

Rebase to D, conflict resolve

  $ hg debugrebaseconflict -r 'max(desc(B))' -d $D
  $ hg debugshowconflict -r 'max(desc(B))'
  d43f25602f42: no conflict
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  │
  o  D
  │
  │ o  C
  ├─╯
  o  A
  
TODO: Conflict on conflict
