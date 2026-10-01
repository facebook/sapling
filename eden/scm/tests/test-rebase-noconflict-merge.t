#require no-eden

  $ eagerepo
  $ enable amend rebase
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig amend.autorestack=no-conflict
  $ newclientrepo

  $ drawdag <<'EOS'
  > b c   # a/f = 1\n2\n3\n4\n5\n
  > |/    # b/f = one\n2\n3\n4\n5\n
  > a     # c/f = 1\n2\n3\n4\n five\n
  > EOS
  $ sl book -ir $b B
  $ sl book -ir $c C
  $ sl goto -q $b
  $ sl merge -q --noconflict -m m $c
  $ sl book -ir . M
  $ echo d > d
  $ sl commit -Aqm d
  $ sl book -ir . D
  $ showgraph() {
  >   sl log -G -T '{desc} {bookmarks}{extras % "{ifeq(key, "noconflict_merge", " noconflict_merge={value}")}"}{ifeq(p2node, "0000000000000000000000000000000000000000", "", " p1={p1node|short} p2={p2node|short}")}\n' "$@"
  > }
  $ showgraph
  @  d D
  │
  o    m M noconflict_merge=1 p1=35c488ac69be p2=e1f2113a3a9f
  ├─╮
  │ o  c C
  │ │
  o │  b B
  ├─╯
  o  a
  

Amending one parent restacks the merge by merging the new parents again
  $ sl goto -q B
  $ echo bb > b
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "m" (M) (glob)
  rebasing * "d" (D) (glob)
  $ showgraph
  o  d D
  │
  o    m M noconflict_merge=1 p1=0e31471b692b p2=e1f2113a3a9f
  ├─╮
  │ @  b B
  │ │
  o │  c C
  ├─╯
  o  a
  
  $ sl cat -r M b f
  bb
  one
  2
  3
  4
   five

Amending the second parent works as well
  $ sl goto -q C
  $ echo cc > c
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "m" (M) (glob)
  rebasing * "d" (D) (glob)
  $ showgraph
  o  d D
  │
  o    m M noconflict_merge=1 p1=0e31471b692b p2=caf3c47f28be
  ├─╮
  │ @  c C
  │ │
  o │  b B
  ├─╯
  o  a
  

Rebasing the whole stack somewhere else merges the parents again there
  $ sl goto -q $a
  $ echo z > z
  $ sl commit -Aqm z
  $ sl book -ir . Z
  $ sl rebase -r 'B+C+M+D' -d Z
  rebasing * "b" (B) (glob)
  rebasing * "c" (C) (glob)
  rebasing * "m" (M) (glob)
  rebasing * "d" (D) (glob)
  $ showgraph
  o  d D
  │
  o    m M noconflict_merge=1 p1=* p2=* (glob)
  ├─╮
  │ o  c C
  │ │
  o │  b B
  ├─╯
  @  z Z
  │
  o  a
  

When one parent ends up below the other the merge is no longer needed
  $ sl rebase -r C -d B
  rebasing * "c" (C) (glob)
  merging f
  $ sl rebase --restack
  rebasing * "m" (M) (glob)
  note: dropping conflict-free merge *: one parent is now an ancestor of the other (glob)
  rebasing * "d" (D) (glob)
  $ showgraph
  o  d D
  │
  o  c C M
  │
  o  b B
  │
  @  z Z
  │
  o  a
  

On-disk rebase behaves the same
  $ setconfig rebase.experimental.inmemory=False
  $ drawdag <<'EOS'
  > e g   # y/h = 1\n2\n3\n
  > |/    # e/h = one\n2\n3\n
  > y     # g/h = 1\n2\n three\n
  > |
  > Z
  > EOS
  $ sl book -ir $e E
  $ sl book -ir $g G
  $ sl goto -q $e
  $ sl merge -q --noconflict -m n $g
  $ sl book -ir . N
  $ sl goto -q E
  $ echo ee > e
  $ sl amend --no-rebase
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing * "n" (N) (glob)
  merging h
  $ sl log -r N -T '{extras % "{key}={value}\n"}' | grep noconflict
  noconflict_merge=1
  $ sl cat -r N e h
  ee
  one
  2
   three

A conflict between the parents stops the rebase before anything changes
  $ sl goto -q G
  $ printf 'uno\n2\n three\n' > h
  $ sl amend --no-rebase -q
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing * "n" (N) (glob)
  abort: cannot rebase conflict-free merge *: merging * and * would have conflicts in 1 file(s): (glob)
   h
  (amend one of the parents so they merge cleanly, then run 'sl rebase --restack')
  [255]
  $ sl status
  $ sl log -r N -T '{desc} p1={p1node|short} p2={p2node|short}\n'
  n p1=* p2=* (glob)

The same in memory, through amend's automatic restack
  $ setconfig rebase.experimental.inmemory=True
  $ sl goto -q E
  $ echo e2 >> e
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "n" (N) (glob)
  restacking would create conflicts (merging the parents of * again would conflict in h), so you must run it manually (glob)
  (run `sl restack` manually to restack this commit's children)
  $ sl status
  $ sl log -r N -T '{desc} p1={p1node|short} p2={p2node|short}\n'
  n p1=* p2=* (glob)

An explicit --noconflict rebase reports the same way
  $ sl rebase --restack --noconflict
  rebasing * "n" (N) (glob)
  merging the parents of * again would conflict (in h) and --noconflict passed; exiting (glob)

A chain of conflict-free merges is merged again from the bottom up
  $ drawdag <<'EOS'
  > r s t  # r/r = r\n
  >  \|/   # s/s = s\n
  >   Z    # t/t = t\n
  > EOS
  $ sl book -ir $r R
  $ sl book -ir $s S
  $ sl book -ir $t T
  $ sl goto -q R
  $ sl merge -q --noconflict -m m1 S
  $ sl book -ir . M1
  $ sl merge -q --noconflict -m m2 T
  $ sl book -ir . M2
  $ echo u > u
  $ sl commit -Aqm u
  $ sl book -ir . U
  $ sl goto -q R
  $ echo rr > r
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "m1" (M1) (glob)
  rebasing * "m2" (M2) (glob)
  rebasing * "u" (U) (glob)
  $ showgraph -r '(R+S+T)::'
  o  u U
  │
  o    m2 M2 noconflict_merge=1 p1=* p2=* (glob)
  ├─╮
  │ o    m1 M1 noconflict_merge=1 p1=* p2=* (glob)
  │ ├─╮
  │ │ @  r R
  │ │ │
  │ │ ~
  │ │
  o │  t T
  │ │
  ~ │
    │
    o  s S
    │
    ~
  $ sl log -r '(p1(M1) - R) + (p2(M1) - S) + (p1(M2) - M1) + (p2(M2) - T)'
  $ sl cat -r U r s t
  rr
  s
  t

Amending the parent of the outer merge only merges that one again
  $ sl goto -q T
  $ echo tt > t
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "m2" (M2) (glob)
  rebasing * "u" (U) (glob)
  $ sl log -r '(p1(M1) - R) + (p2(M1) - S) + (p1(M2) - M1) + (p2(M2) - T)'
  $ sl cat -r U r s t
  rr
  s
  tt

One side of the merge landed: its local commit is obsolete with a public
successor. W1 and W2 stand for warm bookmarks that do not contain the landed
commit, MASTER for master after it.
  $ drawdag <<'EOS'
  > j k    # j/j = j\n
  > |/     # k/k = k\n
  > Z
  > EOS
  $ sl book -ir $j J
  $ sl book -ir $k K
  $ sl goto -q J
  $ sl merge -q --noconflict -m o K
  $ sl book -ir . O
  $ echo q > q
  $ sl commit -Aqm q
  $ sl book -ir . Q
  $ sl goto -q --inactive Z
  $ echo l > l
  $ sl commit -Aqm l
  $ sl rebase -q -r K -d .
  $ sl goto -q --inactive K
  $ echo l2 > l2
  $ sl commit -Aqm l2
  $ sl book -ir . MASTER
  $ for w in W1 W2; do sl goto -q --inactive Z; echo $w > $w; sl commit -Aqm $w; sl book -ir . $w; done
  $ sl debugmakepublic MASTER W1 W2

Restack leaves the merge alone but still fixes other orphans
  $ sl goto -q --inactive Q
  $ echo p > p
  $ sl commit -Aqm p
  $ sl book -ir . P
  $ sl goto -q Q
  $ echo qq > q
  $ sl amend --no-rebase
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  note: not restacking conflict-free merge *, its parent * landed as * (glob)
  (rebase the stack past the landed commit to drop the merge)
  rebasing * "p" (P) (glob)
  $ sl log -r '(p1(O) - J) + (p2(O) - '$k')'

Amending the other side merges it again with the landed side as it is
  $ sl goto -q J
  $ echo jj > j
  $ sl amend
  restacking children automatically (unless they conflict)
  rebasing * "o" (O) (glob)
  rebasing * "q" (Q) (glob)
  rebasing * "p" (P) (glob)
  $ sl log -r '(p1(O) - J) + (p2(O) - '$k')'

Rebasing the stack onto a warm commit, from the top or from the other side
only, keeps the landed side in the merge
  $ sl goto -q --inactive P
  $ sl rebase -d W1
  rebasing * "j" (J) (glob)
  rebasing * "o" (O) (glob)
  rebasing * "q" (Q) (glob)
  rebasing * "p" (P) (glob)
  $ sl log -r '(p1(O) - J) + (p2(O) - '$k')'
  $ sl cat -r Q j k
  jj
  k
  $ sl rebase -s J -d W2
  rebasing * "j" (J) (glob)
  rebasing * "o" (O) (glob)
  rebasing * "q" (Q) (glob)
  rebasing * "p" (P) (glob)
  $ sl log -r '(p1(O) - J) + (p2(O) - '$k')'

Rebasing only the merge onto a public commit is refused
  $ sl rebase -s O -d MASTER
  rebasing * "o" (O) (glob)
  abort: cannot rebase conflict-free merge *: * is public (glob)
  (rebase the whole stack instead, e.g. 'sl rebase -s * -d *') (glob)
  [255]

Rebasing the stack past the landed commit drops the merge
  $ sl rebase -d MASTER
  rebasing * "j" (J) (glob)
  note: not rebasing * "k", already in destination as * "k" (K) (glob)
  rebasing * "o" (O) (glob)
  note: dropping conflict-free merge *: one parent is now an ancestor of the other (glob)
  rebasing * "q" (Q) (glob)
  rebasing * "p" (P) (glob)
  $ showgraph -r 'MASTER::'
  @  p P
  │
  o  q Q
  │
  o  j J O
  │
  o  l2 MASTER
  │
  ~

A conflict-free merge can also be rebased onto an ancestor of one of its
parents, which replaces that parent; an ancestor of both is ambiguous
  $ drawdag <<'EOS'
  > w i   # v/v = v\n
  > | |   # w/w = w\n
  > v x   # x/x = x\n
  > |/    # i/i = i\n
  > Z
  > EOS
  $ sl book -ir $v V
  $ sl book -ir $w W
  $ sl book -ir $x X
  $ sl book -ir $i I
  $ sl goto -q I
  $ sl merge -q --noconflict -m mi W
  $ sl book -ir . MI
  $ echo top > top
  $ sl commit -Aqm top
  $ sl book -ir . TOP
  $ sl rebase -s MI -d X
  rebasing * "mi" (MI) (glob)
  rebasing * "top" (TOP) (glob)
  $ sl log -r '(p1(MI) - X) + (p2(MI) - W)'
  $ sl rebase -s MI -d V
  rebasing * "mi" (MI) (glob)
  rebasing * "top" (TOP) (glob)
  $ sl log -r '(p1(MI) - X) + (p2(MI) - V)'
  $ sl log -r MI -T '{extras % "{key}={value}\n"}' | grep noconflict
  noconflict_merge=1
  $ sl files -r TOP i v w x top
  i: no such file in rev * (glob)
  w: no such file in rev * (glob)
  top
  v
  x
  $ sl rebase -r MI -d Z
  rebasing * "mi" (MI) (glob)
  abort: cannot rebase * without moving at least one of its parents (glob)
  [255]
  $ sl rebase --abort
  abort: no rebase in progress
  [255]

Parents resolved through separate rewrites can make the merge redundant
  $ newclientrepo
  $ drawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ sl goto -q $b
  $ sl merge -q --noconflict -m m $c
  $ sl book -ir . M
  $ sl goto -q $b
  $ echo bb > b
  $ sl amend -q --no-rebase
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl book -ir . B
  $ sl book -ir $c C
  $ sl rebase -q -r C -d B
  $ sl rebase --restack
  rebasing * "m" (M) (glob)
  note: dropping conflict-free merge *: one parent is now an ancestor of the other (glob)
  $ sl log -r 'M - C' -T '{desc}\n'

Landing after an intermediate draft rewrite leaves the landed side in place
  $ newclientrepo
  $ drawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ sl goto -q $b
  $ sl merge -q --noconflict -m m $c
  $ sl book -ir . M
  $ sl goto -q $c
  $ echo cc > c
  $ sl amend -q --no-rebase
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl book -ir . C
  $ sl goto -q $a
  $ echo landedbase > landedbase
  $ sl commit -Aqm landedbase
  $ sl rebase -q -r C -d .
  $ sl debugmakepublic C
  $ sl goto -q M
  $ sl rebase --restack
  note: not restacking conflict-free merge *, its parent * landed as * (glob)
  (rebase the stack past the landed commit to drop the merge)
  nothing to rebase - empty destination
  $ sl log -r "p2(M) - $c"

A divergent second parent prevents restacking the merge
  $ newclientrepo
  $ drawdag <<'EOS'
  > b c
  > |/
  > a
  > EOS
  $ sl goto -q $c
  $ sl merge -q --noconflict -m m $b
  $ sl book -ir . M
  $ drawdag <<'EOS'
  > b b1 b2 # amend: b -> b1
  >  \|/    # amend: b -> b2
  >   a
  > EOS
  $ sl log -r "heads(successors($b) - obsolete() - public())" -T '{desc}\n'
  b1
  b2
  $ sl goto -q $c
  $ echo cc > c
  $ sl amend -q --no-rebase
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing * "m" (M) (glob)
  abort: cannot rebase conflict-free merge: parent * has multiple draft successors: * (glob)
  (resolve the divergent versions of that parent before restacking)
  [255]
  $ sl rebase --abort
  abort: no rebase in progress
  [255]
  $ sl log -r "p2(M) - $b"

On disk, the commits rebased before a failing re-merge are kept, so fixing a
parent and restacking picks up from them
  $ setconfig rebase.experimental.inmemory=False
  $ drawdag <<'EOS'
  > p q   # v/w = 1\n2\n3\n
  > |/    # p/w = one\n2\n3\n
  > v     # q/w = 1\n2\n three\n
  > |
  > Z
  > EOS
  $ sl book -ir $p PP
  $ sl book -ir $q QQ
  $ sl goto -q PP
  $ sl merge -q --noconflict -m mm QQ
  $ sl book -ir . MM
The config only gates creating such merges; an existing one is still rebased
  $ setconfig experimental.noconflict-merge=false
  $ sl goto -q QQ
  $ printf 'uno\n2\n three\n' > w
  $ sl amend --no-rebase -q
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl goto -q $v
  $ echo vv > v
  $ sl amend --no-rebase -q
  hint[amend-restack]: descendants of * are left behind - use 'sl restack' to rebase them (glob)
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl rebase --restack
  rebasing * "p" (PP) (glob)
  rebasing * "q" (QQ) (glob)
  rebasing * "mm" (MM) (glob)
  abort: cannot rebase conflict-free merge *: merging * and * would have conflicts in 1 file(s): (glob)
   w
  (the commits rebased so far are kept; amend one of the parents so they merge cleanly, then run 'sl rebase --restack')
  [255]
  $ sl rebase --abort
  abort: no rebase in progress
  [255]
  $ sl log -r 'p1(PP) & p1(QQ) & (successors('"$v"') - obsolete())' -T '{desc}\n'
  v
  $ sl goto -q QQ
  $ printf 'one\n2\n three\n' > w
  $ sl amend --no-rebase -q
  $ sl rebase --restack
  rebasing * "mm" (MM) (glob)
  merging w
  $ sl cat -r MM w
  one
  2
   three
