  $ enable amend rebase directaccess
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"
  $ cat >> $HGRCPATH <<EOF
  > [templatealias]
  > sl_mutation_names = dict(amend="Amended as", rebase="Rebased to", split="Split into", fold="Folded into", histedit="Histedited to", rewrite="Rewritten into", land="Landed as")
  > sl_mutations = "{join(mutations % '({get(sl_mutation_names, operation, "Rewritten using {operation} into")} {join(successors % "{node|short}", ", ")})', ' ')}"
  > EOF

Useful functions
  $ tglogm()
  > {
  >   hg log -G -T "{rev}: {node|short} '{desc}' {bookmarks} {sl_mutations}" "$@"
  > }

Test upgrading from obsmarker-based visibility to explicitly tracked visibility.

  $ cd $TESTTMP
  $ newrepo
  $ drawdag << EOS
  > B D F
  > | | |
  > A C E  # amend: A -> C -> E
  >  \|/   # rebase: B -> D -> F
  >   Z
  > EOS
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
Revisions pinned by hiddenoverride are grandfathered in at upgrade time, so pin
a revision to test that.

  $ hg up -q $B
  $ hg up -q $F
  $ tglogm
  @  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | x  2: 917a077edb8d 'B'  (Rewritten into a77c932a84af)
  | |
  | x  1: ac2f7407182b 'A'  (Rewritten into 05eb30556340)
  |/
  o  0: 48b9aae0607f 'Z'
  

Enable visibility tracking.

  $ setconfig visibility.enabled=true
  $ hg debugvisibility start
  $ tglogm
  @  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | x  2: 917a077edb8d 'B'  (Rewritten into a77c932a84af)
  | |
  | x  1: ac2f7407182b 'A'  (Rewritten into 05eb30556340)
  |/
  o  0: 48b9aae0607f 'Z'
  
  $ hg hide $A
  hiding commit ac2f7407182b "A"
  hiding commit 917a077edb8d "B"
  2 changesets hidden
  $ tglogm
  @  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  o  0: 48b9aae0607f 'Z'
  
Pinned revisions continue to get tracked in the background, but commits
that are hidden are only temporarily revealed when updated to.
  $ hg up -q $C
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | @  3: f102e5df2a1d 'C'  (Amended as 05eb30556340)
  |/
  o  0: 48b9aae0607f 'Z'
  
  $ hg up -q $Z
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  @  0: 48b9aae0607f 'Z'
  
  $ tglogm --config visibility.enabled=false
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | x  3: f102e5df2a1d 'C'  (Amended as 05eb30556340)
  |/
  @  0: 48b9aae0607f 'Z'
  
We can also downgrade permanently.

  $ hg debugvisibility stop
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | x  3: f102e5df2a1d 'C'  (Amended as 05eb30556340)
  |/
  @  0: 48b9aae0607f 'Z'
  
Re-upgrading includes the pinned revisions.

  $ hg debugvisibility start
  $ tglogm
  o  6: a77c932a84af 'F'
  |
  o  5: 05eb30556340 'E'
  |
  | x  3: f102e5df2a1d 'C'  (Amended as 05eb30556340)
  |/
  @  0: 48b9aae0607f 'Z'
  
