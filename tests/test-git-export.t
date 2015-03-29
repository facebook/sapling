  $ hg init
  $ echo start > start
  $ hg ci -Amstart
  adding start

New file:

  $ mkdir dir1
  $ echo new > dir1/new
  $ hg ci -Amnew
  adding dir1/new
  $ hg diff --git -r 0
  diff --git a/dir1/new b/dir1/new
  new file mode 100644
  --- /dev/null
  +++ b/dir1/new
  @@ -0,0 +1,1 @@
  +new

Copy:

  $ mkdir dir2
  $ hg cp dir1/new dir1/copy
  $ echo copy1 >> dir1/copy
  $ hg cp dir1/new dir2/copy
  $ echo copy2 >> dir2/copy
  $ hg ci -mcopy
  $ hg diff --git -r 1:tip
  diff --git a/dir1/new b/dir1/copy
  copy from dir1/new
  copy to dir1/copy
  --- a/dir1/new
  +++ b/dir1/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  diff --git a/dir1/new b/dir2/copy
  copy from dir1/new
  copy to dir2/copy
  --- a/dir1/new
  +++ b/dir2/copy
  @@ -1,1 +1,2 @@
   new
  +copy2

Cross and same-directory copies with a relative root:

  $ hg diff --git --root .. -r 1:tip
  abort: .. not under root '$TESTTMP'
  [255]
  $ hg diff --git --root doesnotexist -r 1:tip
  $ hg diff --git --root . -r 1:tip
  diff --git a/dir1/new b/dir1/copy
  copy from dir1/new
  copy to dir1/copy
  --- a/dir1/new
  +++ b/dir1/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  diff --git a/dir1/new b/dir2/copy
  copy from dir1/new
  copy to dir2/copy
  --- a/dir1/new
  +++ b/dir2/copy
  @@ -1,1 +1,2 @@
   new
  +copy2
  $ hg diff --git --root dir1 -r 1:tip
  diff --git a/new b/copy
  copy from new
  copy to copy
  --- a/new
  +++ b/copy
  @@ -1,1 +1,2 @@
   new
  +copy1

  $ hg diff --git --root dir2/ -r 1:tip
  diff --git a/copy b/copy
  new file mode 100644
  --- /dev/null
  +++ b/copy
  @@ -0,0 +1,2 @@
  +new
  +copy2

  $ hg diff --git --root dir1 -r 1:tip -I '**/copy'
  diff --git a/new b/copy
  copy from new
  copy to copy
  --- a/new
  +++ b/copy
  @@ -1,1 +1,2 @@
   new
  +copy1

  $ hg diff --git --root dir1 -r 1:tip dir2
  warning: dir2 not inside relative root dir1

  $ hg diff --git --root dir1 -r 1:tip 'dir2/{copy}'
  warning: dir2/{copy} not inside relative root dir1 (glob)

  $ cd dir1
  $ hg diff --git --root .. -r 1:tip
  diff --git a/dir1/new b/dir1/copy
  copy from dir1/new
  copy to dir1/copy
  --- a/dir1/new
  +++ b/dir1/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  diff --git a/dir1/new b/dir2/copy
  copy from dir1/new
  copy to dir2/copy
  --- a/dir1/new
  +++ b/dir2/copy
  @@ -1,1 +1,2 @@
   new
  +copy2

  $ hg diff --git --root ../.. -r 1:tip
  abort: ../.. not under root '$TESTTMP'
  [255]
  $ hg diff --git --root ../doesnotexist -r 1:tip
  $ hg diff --git --root .. -r 1:tip
  diff --git a/dir1/new b/dir1/copy
  copy from dir1/new
  copy to dir1/copy
  --- a/dir1/new
  +++ b/dir1/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  diff --git a/dir1/new b/dir2/copy
  copy from dir1/new
  copy to dir2/copy
  --- a/dir1/new
  +++ b/dir2/copy
  @@ -1,1 +1,2 @@
   new
  +copy2

  $ hg diff --git --root . -r 1:tip
  diff --git a/new b/copy
  copy from new
  copy to copy
  --- a/new
  +++ b/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  $ hg diff --git --root . -r 1:tip copy
  diff --git a/new b/copy
  copy from new
  copy to copy
  --- a/new
  +++ b/copy
  @@ -1,1 +1,2 @@
   new
  +copy1
  $ hg diff --git --root . -r 1:tip ../dir2
  warning: ../dir2 not inside relative root . (glob)
  $ hg diff --git --root . -r 1:tip '../dir2/*'
  warning: ../dir2/* not inside relative root . (glob)
  $ cd ..

Rename:

  $ hg mv dir1/copy dir1/rename1
  $ echo rename1 >> dir1/rename1
  $ hg mv dir2/copy dir1/rename2
  $ echo rename2 >> dir1/rename2
  $ hg ci -mrename
  $ hg diff --git -r 2:tip
  diff --git a/dir1/copy b/dir1/rename1
  rename from dir1/copy
  rename to dir1/rename1
  --- a/dir1/copy
  +++ b/dir1/rename1
  @@ -1,2 +1,3 @@
   new
   copy1
  +rename1
  diff --git a/dir2/copy b/dir1/rename2
  rename from dir2/copy
  rename to dir1/rename2
  --- a/dir2/copy
  +++ b/dir1/rename2
  @@ -1,2 +1,3 @@
   new
   copy2
  +rename2

Cross and same-directory renames with a relative root:

  $ hg diff --root dir1 --git -r 2:tip
  diff --git a/copy b/rename1
  rename from copy
  rename to rename1
  --- a/copy
  +++ b/rename1
  @@ -1,2 +1,3 @@
   new
   copy1
  +rename1
  diff --git a/rename2 b/rename2
  new file mode 100644
  --- /dev/null
  +++ b/rename2
  @@ -0,0 +1,3 @@
  +new
  +copy2
  +rename2

  $ hg diff --root dir2 --git -r 2:tip
  diff --git a/copy b/copy
  deleted file mode 100644
  --- a/copy
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -new
  -copy2

  $ hg diff --root dir1 --git -r 2:tip -I '**/copy'
  diff --git a/copy b/copy
  deleted file mode 100644
  --- a/copy
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -new
  -copy1

  $ hg diff --root dir1 --git -r 2:tip -I '**/rename*'
  diff --git a/copy b/rename1
  copy from copy
  copy to rename1
  --- a/copy
  +++ b/rename1
  @@ -1,2 +1,3 @@
   new
   copy1
  +rename1
  diff --git a/rename2 b/rename2
  new file mode 100644
  --- /dev/null
  +++ b/rename2
  @@ -0,0 +1,3 @@
  +new
  +copy2
  +rename2

Delete:

  $ hg rm dir1/*
  $ hg ci -mdelete
  $ hg diff --git -r 3:tip
  diff --git a/dir1/new b/dir1/new
  deleted file mode 100644
  --- a/dir1/new
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -new
  diff --git a/dir1/rename1 b/dir1/rename1
  deleted file mode 100644
  --- a/dir1/rename1
  +++ /dev/null
  @@ -1,3 +0,0 @@
  -new
  -copy1
  -rename1
  diff --git a/dir1/rename2 b/dir1/rename2
  deleted file mode 100644
  --- a/dir1/rename2
  +++ /dev/null
  @@ -1,3 +0,0 @@
  -new
  -copy2
  -rename2

  $ cat > src <<EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg ci -Amsrc
  adding src

#if execbit

chmod 644:

  $ chmod +x src
  $ hg ci -munexec
  $ hg diff --git -r 5:tip
  diff --git a/src b/src
  old mode 100644
  new mode 100755

Rename+mod+chmod:

  $ hg mv src dst
  $ chmod -x dst
  $ echo a >> dst
  $ hg ci -mrenamemod
  $ hg diff --git -r 6:tip
  diff --git a/src b/dst
  old mode 100755
  new mode 100644
  rename from src
  rename to dst
  --- a/src
  +++ b/dst
  @@ -3,3 +3,4 @@
   3
   4
   5
  +a

Nonexistent in tip+chmod:

  $ hg diff --git -r 5:6
  diff --git a/src b/src
  old mode 100644
  new mode 100755

#else

Dummy changes when no exec bit, mocking the execbit commit structure

  $ echo change >> src
  $ hg ci -munexec
  $ hg mv src dst
  $ hg ci -mrenamemod

#endif

Binary diff:

  $ cp "$TESTDIR/binfile.bin" .
  $ hg add binfile.bin
  $ hg diff --git > b.diff
  $ cat b.diff
  diff --git a/binfile.bin b/binfile.bin
  new file mode 100644
  index e69de29bb2d1d6434b8b29ae775ad8c2e48c5391..37ba3d1c6f17137d9c5f5776fa040caf5fe73ff9
  GIT binary patch
  literal 593
  zc$@)I0<QguP)<h;3K|Lk000e1NJLTq000mG000mO0ssI2kdbIM00009a7bBm000XU
  z000XU0RWnu7ytkO2XskIMF-Uh9TW;VpMjwv0005-Nkl<ZD9@FWPs=e;7{<>W$NUkd
  zX$nnYLt$-$V!?uy+1V%`z&Eh=ah|duER<4|QWhju3gb^nF*8iYobxWG-qqXl=2~5M
  z*IoDB)sG^CfNuoBmqLTVU^<;@nwHP!1wrWd`{(mHo6VNXWtyh{alzqmsH*yYzpvLT
  zLdY<T=ks|woh-`&01!ej#(xbV1f|pI*=%;d-%F*E*X#ZH`4I%6SS+$EJDE&ct=8po
  ziN#{?_j|kD%Cd|oiqds`xm@;oJ-^?NG3Gdqrs?5u*zI;{nogxsx~^|Fn^Y?Gdc6<;
  zfMJ+iF1J`LMx&A2?dEwNW8ClebzPTbIh{@$hS6*`kH@1d%Lo7fA#}N1)oN7`gm$~V
  z+wDx#)OFqMcE{s!JN0-xhG8ItAjVkJwEcb`3WWlJfU2r?;Pd%dmR+q@mSri5q9_W-
  zaR2~ECX?B2w+zELozC0s*6Z~|QG^f{3I#<`?)Q7U-JZ|q5W;9Q8i_=pBuSzunx=U;
  z9C)5jBoYw9^?EHyQl(M}1OlQcCX>lXB*ODN003Z&P17_@)3Pi=i0wb04<W?v-u}7K
  zXmmQA+wDgE!qR9o8jr`%=ab_&uh(l?R=r;Tjiqon91I2-hIu?57~@*4h7h9uORK#=
  fQItJW-{SoTm)8|5##k|m00000NkvXXu0mjf{mKw4
  

Import binary diff:

  $ hg revert binfile.bin
  $ rm binfile.bin
  $ hg import -mfoo b.diff
  applying b.diff
  $ cmp binfile.bin "$TESTDIR/binfile.bin"

Rename binary file:

  $ hg mv binfile.bin renamed.bin
  $ hg diff --git
  diff --git a/binfile.bin b/renamed.bin
  rename from binfile.bin
  rename to renamed.bin

Diff across many revisions:

  $ hg mv dst dst2
  $ hg ci -m 'mv dst dst2'

  $ echo >> start
  $ hg ci -m 'change start'

  $ hg revert -r -2 start
  $ hg mv dst2 dst3
  $ hg ci -m 'mv dst2 dst3; revert start'

  $ hg diff --git -r 9:11
  diff --git a/dst2 b/dst3
  rename from dst2
  rename to dst3

Reversed:

  $ hg diff --git -r 11:9
  diff --git a/dst3 b/dst2
  rename from dst3
  rename to dst2


  $ echo a >> foo
  $ hg add foo
  $ hg ci -m 'add foo'
  $ echo b >> foo
  $ hg ci -m 'change foo'
  $ hg mv foo bar
  $ hg ci -m 'mv foo bar'
  $ echo c >> bar
  $ hg ci -m 'change bar'

File created before r1 and renamed before r2:

  $ hg diff --git -r -3:-1
  diff --git a/foo b/bar
  rename from foo
  rename to bar
  --- a/foo
  +++ b/bar
  @@ -1,2 +1,3 @@
   a
   b
  +c

Reversed:

  $ hg diff --git -r -1:-3
  diff --git a/bar b/foo
  rename from bar
  rename to foo
  --- a/bar
  +++ b/foo
  @@ -1,3 +1,2 @@
   a
   b
  -c

File created in r1 and renamed before r2:

  $ hg diff --git -r -4:-1
  diff --git a/foo b/bar
  rename from foo
  rename to bar
  --- a/foo
  +++ b/bar
  @@ -1,1 +1,3 @@
   a
  +b
  +c

Reversed:

  $ hg diff --git -r -1:-4
  diff --git a/bar b/foo
  rename from bar
  rename to foo
  --- a/bar
  +++ b/foo
  @@ -1,3 +1,1 @@
   a
  -b
  -c

File created after r1 and renamed before r2:

  $ hg diff --git -r -5:-1
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,3 @@
  +a
  +b
  +c

Reversed:

  $ hg diff --git -r -1:-5
  diff --git a/bar b/bar
  deleted file mode 100644
  --- a/bar
  +++ /dev/null
  @@ -1,3 +0,0 @@
  -a
  -b
  -c


Comparing with the working dir:

  $ echo >> start
  $ hg ci -m 'change start again'

  $ echo > created
  $ hg add created
  $ hg ci -m 'add created'

  $ hg mv created created2
  $ hg ci -m 'mv created created2'

  $ hg mv created2 created3

There's a copy in the working dir:

  $ hg diff --git
  diff --git a/created2 b/created3
  rename from created2
  rename to created3

There's another copy between the original rev and the wd:

  $ hg diff --git -r -2
  diff --git a/created b/created3
  rename from created
  rename to created3

The source of the copy was created after the original rev:

  $ hg diff --git -r -3
  diff --git a/created3 b/created3
  new file mode 100644
  --- /dev/null
  +++ b/created3
  @@ -0,0 +1,1 @@
  +
  $ hg ci -m 'mv created2 created3'


  $ echo > brand-new
  $ hg add brand-new
  $ hg ci -m 'add brand-new'
  $ hg mv brand-new brand-new2

Created in parent of wd; renamed in the wd:

  $ hg diff --git
  diff --git a/brand-new b/brand-new2
  rename from brand-new
  rename to brand-new2

Created between r1 and parent of wd; renamed in the wd:

  $ hg diff --git -r -2
  diff --git a/brand-new2 b/brand-new2
  new file mode 100644
  --- /dev/null
  +++ b/brand-new2
  @@ -0,0 +1,1 @@
  +
  $ hg ci -m 'mv brand-new brand-new2'

One file is copied to many destinations and removed:

  $ hg cp brand-new2 brand-new3
  $ hg mv brand-new2 brand-new3-2
  $ hg ci -m 'multiple renames/copies'
  $ hg diff --git -r -2 -r -1
  diff --git a/brand-new2 b/brand-new3
  rename from brand-new2
  rename to brand-new3
  diff --git a/brand-new2 b/brand-new3-2
  copy from brand-new2
  copy to brand-new3-2

Reversed:

  $ hg diff --git -r -1 -r -2
  diff --git a/brand-new3-2 b/brand-new2
  rename from brand-new3-2
  rename to brand-new2
  diff --git a/brand-new3 b/brand-new3
  deleted file mode 100644
  --- a/brand-new3
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -

There should be a trailing TAB if there are spaces in the file name:

  $ echo foo > 'with spaces'
  $ hg add 'with spaces'
  $ hg diff --git
  diff --git a/with spaces b/with spaces
  new file mode 100644
  --- /dev/null
  +++ b/with spaces	
  @@ -0,0 +1,1 @@
  +foo
  $ hg ci -m 'add filename with spaces'

Additions should be properly marked even in the middle of a merge

  $ hg up -r -2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "New File" >> inmerge
  $ hg add inmerge
  $ hg ci -m "file in merge"
  created new head
  $ hg up 23
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg diff -g
  diff --git a/inmerge b/inmerge
  new file mode 100644
  --- /dev/null
  +++ b/inmerge
  @@ -0,0 +1,1 @@
  +New File
