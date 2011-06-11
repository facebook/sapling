
  $ hg init

New file:

  $ hg import -d "1000000 0" -mnew - <<EOF
  > diff --git a/new b/new
  > new file mode 100644
  > index 0000000..7898192
  > --- /dev/null
  > +++ b/new
  > @@ -0,0 +1 @@
  > +a
  > EOF
  applying patch from stdin

  $ hg tip -q
  0:ae3ee40d2079

New empty file:

  $ hg import -d "1000000 0" -mempty - <<EOF
  > diff --git a/empty b/empty
  > new file mode 100644
  > EOF
  applying patch from stdin

  $ hg tip -q
  1:ab199dc869b5

  $ hg locate empty
  empty

chmod +x:

  $ hg import -d "1000000 0" -msetx - <<EOF
  > diff --git a/new b/new
  > old mode 100644
  > new mode 100755
  > EOF
  applying patch from stdin

  $ hg tip -q
  2:3a34410f282e

  $ test -x new

Copy:

  $ hg import -d "1000000 0" -mcopy - <<EOF
  > diff --git a/new b/copy
  > old mode 100755
  > new mode 100644
  > similarity index 100%
  > copy from new
  > copy to copy
  > diff --git a/new b/copyx
  > similarity index 100%
  > copy from new
  > copy to copyx
  > EOF
  applying patch from stdin

  $ hg tip -q
  3:37bacb7ca14d

  $ if "$TESTDIR/hghave" -q execbit; then
  >     test -f copy -a ! -x copy || echo bad
  >     test -x copyx || echo bad
  > else
  >     test -f copy || echo bad
  > fi

  $ cat copy
  a

  $ hg cat copy
  a

Rename:

  $ hg import -d "1000000 0" -mrename - <<EOF
  > diff --git a/copy b/rename
  > similarity index 100%
  > rename from copy
  > rename to rename
  > EOF
  applying patch from stdin

  $ hg tip -q
  4:47b81a94361d

  $ hg locate
  copyx
  empty
  new
  rename

Delete:

  $ hg import -d "1000000 0" -mdelete - <<EOF
  > diff --git a/copyx b/copyx
  > deleted file mode 100755
  > index 7898192..0000000
  > --- a/copyx
  > +++ /dev/null
  > @@ -1 +0,0 @@
  > -a
  > EOF
  applying patch from stdin

  $ hg tip -q
  5:d9b001d98336

  $ hg locate
  empty
  new
  rename

  $ test -f copyx
  [1]

Regular diff:

  $ hg import -d "1000000 0" -mregular - <<EOF
  > diff --git a/rename b/rename
  > index 7898192..72e1fe3 100644
  > --- a/rename
  > +++ b/rename
  > @@ -1 +1,5 @@
  >  a
  > +a
  > +a
  > +a
  > +a
  > EOF
  applying patch from stdin

  $ hg tip -q
  6:ebe901e7576b

Copy and modify:

  $ hg import -d "1000000 0" -mcopymod - <<EOF
  > diff --git a/rename b/copy2
  > similarity index 80%
  > copy from rename
  > copy to copy2
  > index 72e1fe3..b53c148 100644
  > --- a/rename
  > +++ b/copy2
  > @@ -1,5 +1,5 @@
  >  a
  >  a
  > -a
  > +b
  >  a
  >  a
  > EOF
  applying patch from stdin

  $ hg tip -q
  7:18f368958ecd

  $ hg cat copy2
  a
  a
  b
  a
  a

Rename and modify:

  $ hg import -d "1000000 0" -mrenamemod - <<EOF
  > diff --git a/copy2 b/rename2
  > similarity index 80%
  > rename from copy2
  > rename to rename2
  > index b53c148..8f81e29 100644
  > --- a/copy2
  > +++ b/rename2
  > @@ -1,5 +1,5 @@
  >  a
  >  a
  >  b
  > -a
  > +c
  >  a
  > EOF
  applying patch from stdin

  $ hg tip -q
  8:c32b0d7e6f44

  $ hg locate copy2
  [1]
  $ hg cat rename2
  a
  a
  b
  c
  a

One file renamed multiple times:

  $ hg import -d "1000000 0" -mmultirenames - <<EOF
  > diff --git a/rename2 b/rename3
  > rename from rename2
  > rename to rename3
  > diff --git a/rename2 b/rename3-2
  > rename from rename2
  > rename to rename3-2
  > EOF
  applying patch from stdin

  $ hg tip -q
  9:034a6bf95330

  $ hg log -vr. --template '{rev} {files} / {file_copies}\n'
  9 rename2 rename3 rename3-2 / rename3 (rename2)rename3-2 (rename2)

  $ hg locate rename2 rename3 rename3-2
  rename3
  rename3-2

  $ hg cat rename3
  a
  a
  b
  c
  a

  $ hg cat rename3-2
  a
  a
  b
  c
  a

  $ echo foo > foo
  $ hg add foo
  $ hg ci -m 'add foo'

Binary files and regular patch hunks:

  $ hg import -d "1000000 0" -m binaryregular - <<EOF
  > diff --git a/binary b/binary
  > new file mode 100644
  > index 0000000000000000000000000000000000000000..593f4708db84ac8fd0f5cc47c634f38c013fe9e4
  > GIT binary patch
  > literal 4
  > Lc\${NkU|;|M00aO5
  > 
  > diff --git a/foo b/foo2
  > rename from foo
  > rename to foo2
  > EOF
  applying patch from stdin

  $ hg tip -q
  11:c39bce63e786

  $ cat foo2
  foo

  $ hg manifest --debug | grep binary
  045c85ba38952325e126c70962cc0f9d9077bc67 644   binary

Multiple binary files:

  $ hg import -d "1000000 0" -m multibinary - <<EOF
  > diff --git a/mbinary1 b/mbinary1
  > new file mode 100644
  > index 0000000000000000000000000000000000000000..593f4708db84ac8fd0f5cc47c634f38c013fe9e4
  > GIT binary patch
  > literal 4
  > Lc\${NkU|;|M00aO5
  > 
  > diff --git a/mbinary2 b/mbinary2
  > new file mode 100644
  > index 0000000000000000000000000000000000000000..112363ac1917b417ffbd7f376ca786a1e5fa7490
  > GIT binary patch
  > literal 5
  > Mc\${NkU|\`?^000jF3jhEB
  > 
  > EOF
  applying patch from stdin

  $ hg tip -q
  12:30b530085242

  $ hg manifest --debug | grep mbinary
  045c85ba38952325e126c70962cc0f9d9077bc67 644   mbinary1
  a874b471193996e7cb034bb301cac7bdaf3e3f46 644   mbinary2

Filenames with spaces:

  $ hg import -d "1000000 0" -m spaces - <<EOF
  > diff --git a/foo bar b/foo bar
  > new file mode 100644
  > index 0000000..257cc56
  > --- /dev/null
  > +++ b/foo bar	
  > @@ -0,0 +1 @@
  > +foo
  > EOF
  applying patch from stdin

  $ hg tip -q
  13:04750ef42fb3

  $ cat "foo bar"
  foo

Copy then modify the original file:

  $ hg import -d "1000000 0" -m copy-mod-orig - <<EOF
  > diff --git a/foo2 b/foo2
  > index 257cc56..fe08ec6 100644
  > --- a/foo2
  > +++ b/foo2
  > @@ -1 +1,2 @@
  >  foo
  > +new line
  > diff --git a/foo2 b/foo3
  > similarity index 100%
  > copy from foo2
  > copy to foo3
  > EOF
  applying patch from stdin

  $ hg tip -q
  14:c4cd9cdeaa74

  $ cat foo3
  foo

Move text file and patch as binary

  $ echo a > text2
  $ hg ci -Am0
  adding text2
  $ hg import -d "1000000 0" -m rename-as-binary - <<"EOF"
  > diff --git a/text2 b/binary2
  > rename from text2
  > rename to binary2
  > index 78981922613b2afb6025042ff6bd878ac1994e85..10efcb362e9f3b3420fcfbfc0e37f3dc16e29757
  > GIT binary patch
  > literal 5
  > Mc$`b*O5$Pw00T?_*Z=?k
  > 
  > EOF
  applying patch from stdin

  $ cat binary2
  a
  b
  \x00 (no-eol) (esc)

  $ hg st --copies --change . 
  A binary2
    text2
  R text2
  $ cd ..

Consecutive import with renames (issue2459)

  $ hg init issue2459
  $ cd issue2459
  $ hg import --no-commit --force - <<EOF
  > diff --git a/a b/a
  > new file mode 100644
  > EOF
  applying patch from stdin
  $ hg import --no-commit --force - <<EOF
  > diff --git a/a b/b
  > rename from a
  > rename to b
  > EOF
  applying patch from stdin
  a has not been committed yet, so no copy data will be stored for b.
  $ hg debugstate
  a   0         -1 unset               b
  $ hg ci -m done
  $ cd ..

Renames and strip

  $ hg init renameandstrip
  $ cd renameandstrip
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg import --no-commit -p2 - <<EOF
  > diff --git a/foo/a b/foo/b
  > rename from foo/a
  > rename to foo/b
  > EOF
  applying patch from stdin
  $ hg st --copies
  A b
    a
  R a
  $ cd ..

Pure copy with existing destination

  $ hg init copytoexisting
  $ cd copytoexisting
  $ echo a > a
  $ echo b > b
  $ hg ci -Am add
  adding a
  adding b
  $ hg import --no-commit - <<EOF
  > diff --git a/a b/b
  > copy from a
  > copy to b
  > EOF
  applying patch from stdin
  abort: cannot create b: destination already exists
  [255]
  $ cat b
  b

Copy and changes with existing destination

  $ hg import --no-commit - <<EOF
  > diff --git a/a b/b
  > copy from a
  > copy to b
  > --- a/a
  > +++ b/b
  > @@ -1,1 +1,2 @@
  > a
  > +b
  > EOF
  applying patch from stdin
  cannot create b: destination already exists
  1 out of 1 hunks FAILED -- saving rejects to file b.rej
  abort: patch failed to apply
  [255]
  $ cat b
  b

  $ ln -s b linkb
  $ hg add linkb
  $ hg ci -m addlinkb
  $ hg import --no-commit - <<EOF
  > diff --git a/linkb b/linkb
  > deleted file mode 120000
  > --- a/linkb
  > +++ /dev/null
  > @@ -1,1 +0,0 @@
  > -badhunk
  > \ No newline at end of file
  > EOF
  applying patch from stdin
  patching file linkb
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file linkb.rej
  abort: patch failed to apply
  [255]
  $ hg st
  ? b.rej
  ? linkb.rej

  $ cd ..
