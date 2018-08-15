Set up test environment.
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fbamend=
  > inhibit=
  > rebase=
  > strip=
  > [experimental]
  > evolution = createmarkers, allowunstable
  > [ui]
  > interactive = true
  > [fbamend]
  > safestrip = false
  > EOF
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
  @  3 add d1 and d2
  |
  o  2 add c1 and c2
  |
  o  1 add b1 and b2
  |
  o  0 add a1 and a2

Test that split behaves correctly on error.
  $ hg split -r 0 1 2
  abort: more than one revset is given
  (use either `hg split <rs>` or `hg split --rev <rs>`, not both)
  [255]

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
  @  5 add d1 and d2
  |
  o  4 add d1 and d2
  |
  o  2 add c1 and c2
  |
  o  1 add b1 and b2
  |
  o  0 add a1 and a2

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
  rebasing 4:* "add d1 and d2" (glob)
  rebasing 5:* "add d1 and d2"* (glob)

  $ showgraph
  o  9 add d1 and d2
  |
  o  8 add d1 and d2
  |
  @  7 add c1 and c2
  |
  o  6 add c1 and c2
  |
  o  1 add b1 and b2
  |
  o  0 add a1 and a2

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
  rebasing 1:* "add b1 and b2" (glob)
  rebasing 6:* "add c1 and c2" (glob)
  rebasing 7:* "add c1 and c2" (glob)
  rebasing 8:* "add d1 and d2" (glob)
  rebasing 9:* "add d1 and d2" (glob)
  rebasing 10:* "add d1 and d2"* (glob)

  $ showgraph
  o  18 add d1 and d2
  |
  | o  17 add d1 and d2
  | |
  | o  16 add d1 and d2
  | |
  | o  15 add c1 and c2
  | |
  | o  14 add c1 and c2
  | |
  | o  13 add b1 and b2
  |/
  @  12 add a1 and a2
  |
  o  11 add a1 and a2

Split using revset.
  $ hg strip 18
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
  rebasing 14:* "add c1 and c2" (glob)
  rebasing 15:* "add c1 and c2" (glob)
  rebasing 16:* "add d1 and d2" (glob)
  rebasing 17:* "add d1 and d2"* (glob)

  $ showgraph
  o  23 add d1 and d2
  |
  o  22 add d1 and d2
  |
  o  21 add c1 and c2
  |
  o  20 add c1 and c2
  |
  @  19 add b1 and b2
  |
  o  18 add b1 and b2
  |
  o  12 add a1 and a2
  |
  o  11 add a1 and a2
  
  o  10 add d1 and d2
  |
  x  0 add a1 and a2

Test that command aborts when given multiple commits.
  $ hg split 11 12
  abort: more than one revset is given
  (use either `hg split <rs>` or `hg split --rev <rs>`, not both)
  [255]

Test --no-rebase flag.
  $ mkcommit e
  $ hg rebase -s 20 -d .
  rebasing 20:* "add c1 and c2" (glob)
  rebasing 21:* "add c1 and c2" (glob)
  rebasing 22:* "add d1 and d2" (glob)
  rebasing 23:* "add d1 and d2" (glob)
  $ showgraph
  o  28 add d1 and d2
  |
  o  27 add d1 and d2
  |
  o  26 add c1 and c2
  |
  o  25 add c1 and c2
  |
  @  24 add e1 and e2
  |
  o  19 add b1 and b2
  |
  o  18 add b1 and b2
  |
  o  12 add a1 and a2
  |
  o  11 add a1 and a2
  
  o  10 add d1 and d2
  |
  x  0 add a1 and a2
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
  @  30 add e1 and e2
  |
  o  29 add e1 and e2
  |
  | o  28 add d1 and d2
  | |
  | o  27 add d1 and d2
  | |
  | o  26 add c1 and c2
  | |
  | o  25 add c1 and c2
  | |
  | x  24 add e1 and e2
  |/
  o  19 add b1 and b2
  |
  o  18 add b1 and b2
  |
  o  12 add a1 and a2
  |
  o  11 add a1 and a2
  
  o  10 add d1 and d2
  |
  x  0 add a1 and a2

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
  @  2 add a1 and a2
  |
  o  1 add a1 and a2
  $ hg bookmarks
     test1                     2:* (glob)
   * test2                     2:* (glob)
