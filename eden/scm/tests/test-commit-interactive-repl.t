#require no-eden

  $ eagerepo
  $ setconfig ui.interactive=true ui.interface.chunkselector=repl
  $ newclientrepo repo

Create two files. The changes to a produce two independently selectable hunks.

  $ cat > a <<'EOF'
  > one
  > two
  > three
  > EOF
  $ echo base > b
  $ sl commit -Aqm base
  $ cat > a <<'EOF'
  > ONE
  > two
  > THREE
  > EOF
  $ cat > b <<'EOF'
  > base
  > added
  > EOF

EOF aborts the selection cleanly.

  $ sl commit -i -m eof </dev/null >$TESTTMP/eof-output 2>&1
  [255]
  $ tail -n 1 $TESTTMP/eof-output
  chunk-select> abort: chunk selection aborted

Selector errors do not partially apply a comma-separated command. Select only
the first replacement in a, then preview and commit it.

  $ sl commit -i -m partial <<'EOF'
  > show F1.H1.L1-2
  > select F2,F9
  > select F1.H1.L1
  > deselect F1.H1.L1
  > select F1.H1.L1-2
  > show F1
  > preview
  > confirm
  > EOF
  Chunk selector REPL (record). Selection starts empty.
  [ ] F1 'a' (+2 -2)
    [ ] F1.H1 @@ -1,2 +1,2 @@
      [ ] F1.H1.L1 -one
      [ ] F1.H1.L2 +ONE
                    two
    [ ] F1.H2 @@ -2,2 +2,2 @@
                    two
      [ ] F1.H2.L1 -three
      [ ] F1.H2.L2 +THREE
  [ ] F2 'b' (+1 -0)
    [ ] F2.H1 @@ -1,1 +1,2 @@
                    base
      [ ] F2.H1.L1 +added
  Selection: files 0/2, hunks 0/3, lines 0/5
  Commands:
    select SELECTORS    select files, hunks, or changed lines
    deselect SELECTORS  deselect files, hunks, or changed lines
    show SELECTORS      show original patches with selection state
    status              show selection counts
    preview             show the exact selected patch
    confirm             finish selection
    abort               abort the operation
    help                show this help
  
  SELECTORS is a comma-separated set such as F1.H1.L1-3,F2.H2,F4-5.
  Ranges are allowed only on the final component. Use "all" for every file.
  Legend: [ ] none  [~] partial  [x] all
  chunk-select> show F1.H1.L1-2
  [ ] F1 'a' (+2 -2)
    [ ] F1.H1 @@ -1,2 +1,2 @@
      [ ] F1.H1.L1 -one
      [ ] F1.H1.L2 +ONE
                    two
  chunk-select> select F2,F9
  ERROR: F9 is out of range; valid range is F1-2
  chunk-select> select F1.H1.L1
  OK: selected F1.H1.L1
  Selection: files 0/2 (+1 partial), hunks 0/3 (+1 partial), lines 1/5
  chunk-select> deselect F1.H1.L1
  OK: deselected F1.H1.L1
  Selection: files 0/2, hunks 0/3, lines 0/5
  chunk-select> select F1.H1.L1-2
  OK: selected F1.H1.L1-2
  Selection: files 0/2 (+1 partial), hunks 1/3, lines 2/5
  chunk-select> show F1
  [~] F1 'a' (+2 -2)
    [x] F1.H1 @@ -1,2 +1,2 @@
      [x] F1.H1.L1 -one
      [x] F1.H1.L2 +ONE
                    two
    [ ] F1.H2 @@ -2,2 +2,2 @@
                    two
      [ ] F1.H2.L1 -three
      [ ] F1.H2.L2 +THREE
  chunk-select> preview
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,2 @@
  -one
  +ONE
   two
  chunk-select> confirm

  $ sl diff --git -r .^ -r .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
  -one
  +ONE
   two
   three

  $ sl diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
   ONE
   two
  -three
  +THREE
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
   base
  +added

The same selector is used by amend. Select the complete change to b and leave
the other replacement in a in the working copy.

  $ sl amend -i <<'EOF'
  > select F2
  > confirm
  > EOF
  Chunk selector REPL (record). Selection starts empty.
  [ ] F1 'a' (+1 -1)
    [ ] F1.H1 @@ -1,3 +1,3 @@
                    ONE
                    two
      [ ] F1.H1.L1 -three
      [ ] F1.H1.L2 +THREE
  [ ] F2 'b' (+1 -0)
    [ ] F2.H1 @@ -1,1 +1,2 @@
                    base
      [ ] F2.H1.L1 +added
  Selection: files 0/2, hunks 0/2, lines 0/3
  Commands:
    select SELECTORS    select files, hunks, or changed lines
    deselect SELECTORS  deselect files, hunks, or changed lines
    show SELECTORS      show original patches with selection state
    status              show selection counts
    preview             show the exact selected patch
    confirm             finish selection
    abort               abort the operation
    help                show this help
  
  SELECTORS is a comma-separated set such as F1.H1.L1-3,F2.H2,F4-5.
  Ranges are allowed only on the final component. Use "all" for every file.
  Legend: [ ] none  [~] partial  [x] all
  chunk-select> select F2
  OK: selected F2
  Selection: files 1/2, hunks 1/2, lines 1/3
  chunk-select> confirm

  $ sl diff --git -r .^ -r .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
  -one
  +ONE
   two
   three
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
   base
  +added

  $ sl diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
   ONE
   two
  -three
  +THREE

A no-EOL marker follows the changed line automatically and is not assigned a
separate selector.

  $ sl revert a
  $ printf old > noeol
  $ sl commit -Aqm add-noeol
  $ printf new > noeol
  $ sl commit -i -m change-noeol <<'EOF'
  > select F1.H1.L1-2
  > preview
  > confirm
  > EOF
  Chunk selector REPL (record). Selection starts empty.
  [ ] F1 'noeol' (+1 -1)
    [ ] F1.H1 @@ -1,1 +1,1 @@
      [ ] F1.H1.L1 -old
                   \ No newline at end of file
      [ ] F1.H1.L2 +new
                   \ No newline at end of file
  Selection: files 0/1, hunks 0/1, lines 0/2
  Commands:
    select SELECTORS    select files, hunks, or changed lines
    deselect SELECTORS  deselect files, hunks, or changed lines
    show SELECTORS      show original patches with selection state
    status              show selection counts
    preview             show the exact selected patch
    confirm             finish selection
    abort               abort the operation
    help                show this help
  
  SELECTORS is a comma-separated set such as F1.H1.L1-3,F2.H2,F4-5.
  Ranges are allowed only on the final component. Use "all" for every file.
  Legend: [ ] none  [~] partial  [x] all
  chunk-select> select F1.H1.L1-2
  OK: selected F1.H1.L1-2
  Selection: files 1/1, hunks 1/1, lines 2/2
  chunk-select> preview
  diff --git a/noeol b/noeol
  --- a/noeol
  +++ b/noeol
  @@ -1,1 +1,1 @@
  -old
  \ No newline at end of file
  +new
  \ No newline at end of file
  chunk-select> confirm

  $ sl diff --git -r .^ -r .
  diff --git a/noeol b/noeol
  --- a/noeol
  +++ b/noeol
  @@ -1,1 +1,1 @@
  -old
  \ No newline at end of file
  +new
  \ No newline at end of file

Long hunks are folded in the initial display, but show expands them.

  $ seq 101 > large
  $ sl add large
  $ sl commit -i -m folded >$TESTTMP/folded-output 2>&1 <<'EOF'
  > show F1.H1
  > abort
  > EOF
  [255]
  $ grep "lines hidden" $TESTTMP/folded-output
        101 lines hidden; use 'show F1.H1'
  $ grep "F1.H1.L101" $TESTTMP/folded-output | wc -l
  1
