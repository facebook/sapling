#chg-compatible


  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > absorb=
  > EOF

  $ hg init repo1
  $ cd repo1

Make some commits:

  $ for i in 1 2 3; do
  >   echo $i >> a
  >   hg commit -A a -m "commit $i" -q
  > done

absorb --edit-lines will run the editor if filename is provided:

  $ hg absorb --apply-changes --edit-lines
  nothing applied
  [1]
  $ HGEDITOR=cat hg absorb --apply-changes --edit-lines a
  HG: editing a
  HG: "y" means the line to the right exists in the changeset to the top
  HG:
  HG: /---- 4ec16f85269a commit 1
  HG: |/--- 5c5f95224a50 commit 2
  HG: ||/-- 43f0a75bede7 commit 3
  HG: |||
      yyy : 1
       yy : 2
        y : 3
  nothing applied
  [1]

Edit the file using --edit-lines:

  $ cat > editortext << EOF
  >       y : a
  >      yy :  b
  >      y  : c
  >     yy  : d  
  >     y y : e
  >     y   : f
  >     yyy : g
  > EOF
  $ HGEDITOR='cat editortext >' hg absorb -q --apply-changes --edit-lines a
  $ hg cat -r '.^^' a
  d  
  e
  f
  g
  $ hg cat -r '.^' a
   b
  c
  d  
  g
  $ hg cat -r . a
  a
   b
  e
  g
