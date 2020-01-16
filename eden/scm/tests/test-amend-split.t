#chg-compatible

Set up test environment.
  $ enable mutation-norecord amend rebase
  $ setconfig ui.interactive=true amend.safestrip=false hint.ack-hint-ack=true
  $ mkcommit() {
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

Initialize repo.
  $ hg init repo && cd repo
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ mkcommit d
  $ showgraph
  @  3 d86136f6dbff add d1 and d2
  |
  o  2 e5cbbeb3434b add c1 and c2
  |
  o  1 b7fb8fde59b2 add b1 and b2
  |
  o  0 c20cc4d302fc add a1 and a2

Test that split behaves correctly on error.
  $ hg split -r 0 1 2
  abort: more than one revset is given
  (use either `hg split <rs>` or `hg split --rev <rs>`, not both)
  [255]

Test exitting a split early leaves you on the same commit
  $ hg log -r . -T {node}
  d86136f6dbffaed724ce39c03f4028178355246d (no-eol)
  $ hg split << EOF
  > q
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding d1
  adding d2
  diff --git a/d1 b/d1
  new file mode 100644
  examine changes to 'd1'? [Ynesfdaq?] q
  
  abort: user quit
  [255]
  $ hg log -r . -T {node}
  d86136f6dbffaed724ce39c03f4028178355246d (no-eol)

Test basic case of splitting a head.
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
  @  5 43b9beefca2e add d1 and d2
  |
  o  4 3c66f08f0fd3 add d1 and d2
  |
  o  2 e5cbbeb3434b add c1 and c2
  |
  o  1 b7fb8fde59b2 add b1 and b2
  |
  o  0 c20cc4d302fc add a1 and a2

Split in the middle of a stack.
  $ hg up 2
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg split << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding c1
  adding c2
  diff --git a/c1 b/c1
  new file mode 100644
  examine changes to 'c1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +c1
  record change 1/2 to 'c1'? [Ynesfdaq?] y
  
  diff --git a/c2 b/c2
  new file mode 100644
  examine changes to 'c2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  rebasing 3c66f08f0fd3 "add d1 and d2"
  rebasing 43b9beefca2e "add d1 and d2"

  $ showgraph
  o  9 bdb846f063c6 add d1 and d2
  |
  o  8 acbc5d06143b add d1 and d2
  |
  @  7 bff303a2c228 add c1 and c2
  |
  o  6 6227053e403e add c1 and c2
  |
  o  1 b7fb8fde59b2 add b1 and b2
  |
  o  0 c20cc4d302fc add a1 and a2

Split with multiple children and using hash.
  $ hg up c20c
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ mkcommit d
  $ hg split c20c << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  adding a1
  adding a2
  diff --git a/a1 b/a1
  new file mode 100644
  examine changes to 'a1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +a1
  record change 1/2 to 'a1'? [Ynesfdaq?] y
  
  diff --git a/a2 b/a2
  new file mode 100644
  examine changes to 'a2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  rebasing b7fb8fde59b2 "add b1 and b2"
  rebasing 6227053e403e "add c1 and c2"
  rebasing bff303a2c228 "add c1 and c2"
  rebasing acbc5d06143b "add d1 and d2"
  rebasing bdb846f063c6 "add d1 and d2"
  rebasing bd98a3c83a29 "add d1 and d2"

  $ showgraph
  o  18 5ad76779e999 add d1 and d2
  |
  | o  17 c2fa6cc56f60 add d1 and d2
  | |
  | o  16 7c766f705803 add d1 and d2
  | |
  | o  15 7300aee81508 add c1 and c2
  | |
  | o  14 7c57722f849b add c1 and c2
  | |
  | o  13 cf2484b29d75 add b1 and b2
  |/
  @  12 a265b3c6c419 add a1 and a2
  |
  o  11 5a5595e342b1 add a1 and a2

Split using revset.
  $ hg debugstrip 18
  saved backup bundle to * (glob)
  $ hg split "children(.)" << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  adding b1
  adding b2
  diff --git a/b1 b/b1
  new file mode 100644
  examine changes to 'b1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +b1
  record change 1/2 to 'b1'? [Ynesfdaq?] y
  
  diff --git a/b2 b/b2
  new file mode 100644
  examine changes to 'b2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  rebasing 7c57722f849b "add c1 and c2"
  rebasing 7300aee81508 "add c1 and c2"
  rebasing 7c766f705803 "add d1 and d2"
  rebasing c2fa6cc56f60 "add d1 and d2"

  $ showgraph
  o  23 065a5bb834a6 add d1 and d2
  |
  o  22 e3e63b66173e add d1 and d2
  |
  o  21 6b6c4cdbcb5c add c1 and c2
  |
  o  20 216c1cfd66ba add c1 and c2
  |
  @  19 ef9770b15bd8 add b1 and b2
  |
  o  18 172212eeb9e4 add b1 and b2
  |
  o  12 a265b3c6c419 add a1 and a2
  |
  o  11 5a5595e342b1 add a1 and a2

Test that command aborts when given multiple commits.
  $ hg split 11 12
  abort: more than one revset is given
  (use either `hg split <rs>` or `hg split --rev <rs>`, not both)
  [255]

Test --no-rebase flag.
  $ mkcommit e
  $ hg rebase -s 20 -d .
  rebasing 216c1cfd66ba "add c1 and c2"
  rebasing 6b6c4cdbcb5c "add c1 and c2"
  rebasing e3e63b66173e "add d1 and d2"
  rebasing 065a5bb834a6 "add d1 and d2"
  $ showgraph
  o  28 5f8ed24aed8c add d1 and d2
  |
  o  27 b5591417a3eb add d1 and d2
  |
  o  26 484cd1d66520 add c1 and c2
  |
  o  25 e051398780c8 add c1 and c2
  |
  @  24 c1d00dbe112a add e1 and e2
  |
  o  19 ef9770b15bd8 add b1 and b2
  |
  o  18 172212eeb9e4 add b1 and b2
  |
  o  12 a265b3c6c419 add a1 and a2
  |
  o  11 5a5595e342b1 add a1 and a2
  $ hg split --no-rebase << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding e1
  adding e2
  diff --git a/e1 b/e1
  new file mode 100644
  examine changes to 'e1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +e1
  record change 1/2 to 'e1'? [Ynesfdaq?] y
  
  diff --git a/e2 b/e2
  new file mode 100644
  examine changes to 'e2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y

  $ showgraph
  @  30 f98ad9103c3d add e1 and e2
  |
  o  29 cc95492dd94d add e1 and e2
  |
  | o  28 5f8ed24aed8c add d1 and d2
  | |
  | o  27 b5591417a3eb add d1 and d2
  | |
  | o  26 484cd1d66520 add c1 and c2
  | |
  | o  25 e051398780c8 add c1 and c2
  | |
  | x  24 c1d00dbe112a add e1 and e2
  |/
  o  19 ef9770b15bd8 add b1 and b2
  |
  o  18 172212eeb9e4 add b1 and b2
  |
  o  12 a265b3c6c419 add a1 and a2
  |
  o  11 5a5595e342b1 add a1 and a2

Test that bookmarks are correctly moved.
  $ reset
  $ mkcommit a
  $ hg book test1
  $ hg book test2
  $ hg bookmarks
     test1                     0:* (glob)
   * test2                     0:* (glob)
  $ hg split << EOF
  > y
  > y
  > n
  > y
  > EOF
  (leaving bookmark test2)
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding a1
  adding a2
  diff --git a/a1 b/a1
  new file mode 100644
  examine changes to 'a1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +a1
  record change 1/2 to 'a1'? [Ynesfdaq?] y
  
  diff --git a/a2 b/a2
  new file mode 100644
  examine changes to 'a2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y

  $ showgraph
  @  2 a265b3c6c419 add a1 and a2
  |
  o  1 5a5595e342b1 add a1 and a2
  $ hg bookmarks
     test1                     2:* (glob)
   * test2                     2:* (glob)

Test the hint for Phabricator Diffs being duplicated
  $ cd ..
  $ newrepo
  $ echo 1 > a1
  $ echo 2 > a2
  $ hg commit -Aqm "Differential Revision: http://example.com/D1234"
  $ hg split --config split.phabricatoradvice="amend the commit messages to remove them" << EOF
  > y
  > y
  > n
  > y
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  adding a1
  adding a2
  diff --git a/a1 b/a1
  new file mode 100644
  examine changes to 'a1'? [Ynesfdaq?] y
  
  @@ -0,0 +1,1 @@
  +1
  record change 1/2 to 'a1'? [Ynesfdaq?] y
  
  diff --git a/a2 b/a2
  new file mode 100644
  examine changes to 'a2'? [Ynesfdaq?] n
  
  Done splitting? [yN] y
  hint[split-phabricator]: some split commits have the same Phabricator Diff associated with them
  amend the commit messages to remove them
  $ showgraph
  @  2 b696183283c3 Differential Revision: http://example.com/D1234
  |
  o  1 6add538b4b79 Differential Revision: http://example.com/D1234
