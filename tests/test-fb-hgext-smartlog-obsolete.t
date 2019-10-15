  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > histedit=
  > rebase=
  > smartlog=
  > tweakdefaults=
  > [tweakdefaults]
  > histeditkeepdate = true
  > [experimental]
  > evolution = createmarkers, allowunstable
  > evolution.allowdivergence = on
  > [ui]
  > interactive = true
  > EOF
  $ mkcommit() {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "add $1"
  > }
  $ mkcommit2() {
  >    echo "${1}1" > "${1}1"
  >    echo "${1}2" > "${1}2"
  >    hg add "${1}1" "${1}2"
  >    hg ci -m "add ${1}1 and ${1}2"
  > }
  $ reset() {
  >   cd ..
  >   rm -rf repo
  >   hg init repo
  >   cd repo
  > }
  $ showgraph() {
  >   hg log -r "(::.)::" --graph -T "{rev} {desc|firstline}" | sed \$d
  > }
  $ shownodes() {
  >   hg log --graph -T "{rev}:{short(node)} {desc|firstline}" --hidden | sed \$d
  > }
  $ hg init repo && cd repo

Check amend template keyword.
  $ mkcommit a
  $ showgraph
  @  0 add a
  $ hg amend -m "Amended"
  $ showgraph
  @  1 Amended
  $ hg log --hidden -r 0 -T "{desc} {amendsuccessors % '{short(amendsuccessor)}'}\n"
  add a [a-f0-9]* (re)

Check rebase template keyword.
  $ mkcommit b
  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [*] Amended (glob)
  $ mkcommit c
  $ showgraph
  @  3 add c
  |
  | o  2 add b
  |/
  o  1 Amended
  $ hg rebase -r 2 -d .
  rebasing * "add b" (glob)
  $ showgraph
  o  4 add b
  |
  @  3 add c
  |
  o  1 Amended
  $ hg log --hidden -r 2 -T "{desc} {rebasesuccessors % '{short(rebasesuccessor)} '}\n"
  add b [a-f0-9]*  (re)

Check fold template keyword.
  $ hg fold --from 4
  2 changesets folded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ showgraph
  @  5 add c
  |
  o  1 Amended
  $ hg log --hidden -r 3 -T "{desc} {foldsuccessors % '{short(foldsuccessor)} '}\n"
  add c [a-f0-9]*  (re)
  $ hg log --hidden -r 4 -T "{desc} {foldsuccessors % '{short(foldsuccessor)} '}\n"
  add b [a-f0-9]*  (re)

Check split template keyword.
  $ mkcommit2 d
  $ hg split << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding d1
  adding d2
  diff --git a/d1 b/d1
  new file mode 100644
  examine changes to 'd1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +d1
  record change 1/2 to 'd1'? [Ynesfdaq?] y
  
  diff --git a/d2 b/d2
  new file mode 100644
  examine changes to 'd2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  $ showgraph
  @  8 add d1 and d2
  |
  o  7 add d1 and d2
  |
  o  5 add c
  |
  o  1 Amended
  $ hg log --hidden -r 6 -T "{desc} {splitsuccessors % '{short(splitsuccessor)} '}\n"
  add d1 and d2 [a-f0-9]* [a-f0-9]*  (re)

Check histedit template keyword.
  $ reset
  $ hg debugbuilddag -m +6
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ shownodes
  @  5:f2987ebe5838 r5
  |
  o  4:aa70f0fe546a r4
  |
  o  3:cb14eba0ad9c r3
  |
  o  2:f07e66f449d0 r2
  |
  o  1:09bb8c08de89 r1
  |
  o  0:fdaccbb26270 r0
  $ hg histedit --commands - 2>&1 << EOF
  > pick fdaccbb26270
  > drop 09bb8c08de89
  > pick f07e66f449d0
  > fold cb14eba0ad9c
  > pick aa70f0fe546a
  > drop f2987ebe5838
  > EOF
  $ shownodes
  @  9:a14277652442 r4
  |
  o  8:cbe2934dfbab r2
  |
  o  0:fdaccbb26270 r0

Only nodes that were folded or rebased will have successors.
  $ hg log --hidden -r 0 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r0 
  $ hg log --hidden -r 1 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r1 
  $ hg log --hidden -r 2 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r2 cbe2934dfbab 
  $ hg log --hidden -r 3 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r3 cbe2934dfbab 
  $ hg log --hidden -r 4 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r4 a14277652442 
  $ hg log --hidden -r 5 -T "{desc} {histeditsuccessors % '{short(histeditsuccessor)} '}\n"
  r5 

Hidden changesets are not considered as successors
  $ reset
  $ hg debugbuilddag +2
  $ hg log -T '{rev} {node|short}' -G -r 'all()'
  o  1 66f7d451a68b
  |
  o  0 1ea73414a91b
  
  $ hg up tip -q
  $ echo 1 > a
  $ hg commit --amend -m a -A a -d '1 0'

  $ hg up 1 --hidden -q
  $ hg log -T "{rev} {node|short} {amendsuccessors % '(amend as {short(amendsuccessor)}) '}\n" -G -r 'all()'
  o  2 1ef61e92c901
  |
  | @  1 66f7d451a68b (amend as 1ef61e92c901)
  |/
  o  0 1ea73414a91b
  
  $ hg prune 2 -q
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg log -T "{rev} {node|short} {amendsuccessors % '(amend as {short(amendsuccessor)}) '}\n" -G -r 'all()'
  x  1 66f7d451a68b
  |
  @  0 1ea73414a91b
  
