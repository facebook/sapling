#chg-compatible

  $ configure modern
  $ enable dirsync absorb

Prepare the repo
A: in 1st commit, modified at the top
B: in 2nd commit, modified at the top
C: in both commits, modified at the top
D: in both commits, partially absorbed

  $ newrepo
  $ readconfig <<EOF
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/
  > EOF

  $ mkdir dir1

  $ echo A > dir1/A
  $ echo C1 > dir1/C
  $ echo D1 >> dir1/D
  $ hg commit -m A -A dir1/A dir1/C dir1/D
  mirrored adding 'dir1/A' to 'dir2/A'
  mirrored adding 'dir1/C' to 'dir2/C'
  mirrored adding 'dir1/D' to 'dir2/D'

  $ echo B > dir2/B
  $ echo C2 >> dir1/C
  $ echo D2 >> dir1/D
  $ hg commit -m B -A dir2/B dir1/C dir1/D
  mirrored adding 'dir2/B' to 'dir1/B'
  mirrored changes in 'dir1/C' to 'dir2/C'
  mirrored changes in 'dir1/D' to 'dir2/D'

Absorb triggers mirroring

  $ echo A1 > dir2/A
  $ echo B1 > dir1/B
  $ cat << EOF > dir1/C
  > C10
  > C20
  > EOF

  $ cat << EOF > dir1/D
  > D0
  > D1
  > D1.5
  > D2
  > D3
  > EOF

  $ LOG=edenscm::hgext::dirsync=debug hg absorb -a
  showing changes for dir1/B
          @@ -0,1 +0,1 @@
  * -B (glob)
  * +B1 (glob)
  showing changes for dir1/C
          @@ -0,2 +0,2 @@
  * -C1 (glob)
  * -C2 (glob)
  * +C10 (glob)
  * +C20 (glob)
  showing changes for dir1/D
          @@ -0,0 +0,1 @@
  * +D0 (glob)
          @@ -1,0 +2,1 @@
          +D1.5
          @@ -2,0 +4,1 @@
  * +D3 (glob)
  showing changes for dir2/A
          @@ -0,1 +0,1 @@
  * -A (glob)
  * +A1 (glob)
  
  2 changesets affected
  * B (glob)
  * A (glob)
  mirrored adding 'dir1/C' to 'dir2/C'
  mirrored adding 'dir1/D' to 'dir2/D'
  mirrored adding 'dir2/A' to 'dir1/A'
  mirrored adding 'dir1/B' to 'dir2/B'
  mirrored changes in 'dir1/C' to 'dir2/C'
  mirrored changes in 'dir1/D' to 'dir2/D'
  mirrored changes in 'dir2/A' to 'dir1/A'
  DEBUG edenscm::hgext::dirsync: rewrite mirrored dir1/A
  DEBUG edenscm::hgext::dirsync: rewrite mirrored dir2/B
  DEBUG edenscm::hgext::dirsync: rewrite mirrored dir2/C
  DEBUG edenscm::hgext::dirsync: rewrite mirrored dir2/D
  5 of 6 chunks applied

Working copy does not have "M" mirrored files
D has a line not absorbed

  $ hg status
  M dir1/D

  $ hg diff --git
  diff --git a/dir1/D b/dir1/D
  --- a/dir1/D
  +++ b/dir1/D
  @@ -1,4 +1,5 @@
   D0
   D1
  +D1.5
   D2
   D3

Changes are applied

  $ hg log -p -T '{desc}\n' --config diff.git=1
  B
  diff --git a/dir1/B b/dir1/B
  new file mode 100644
  --- /dev/null
  +++ b/dir1/B
  @@ -0,0 +1,1 @@
  +B1
  diff --git a/dir1/C b/dir1/C
  --- a/dir1/C
  +++ b/dir1/C
  @@ -1,1 +1,2 @@
   C10
  +C20
  diff --git a/dir1/D b/dir1/D
  --- a/dir1/D
  +++ b/dir1/D
  @@ -1,2 +1,4 @@
   D0
   D1
  +D2
  +D3
  diff --git a/dir2/B b/dir2/B
  new file mode 100644
  --- /dev/null
  +++ b/dir2/B
  @@ -0,0 +1,1 @@
  +B1
  diff --git a/dir2/C b/dir2/C
  --- a/dir2/C
  +++ b/dir2/C
  @@ -1,1 +1,2 @@
   C10
  +C20
  diff --git a/dir2/D b/dir2/D
  --- a/dir2/D
  +++ b/dir2/D
  @@ -1,2 +1,4 @@
   D0
   D1
  +D2
  +D3
  
  A
  diff --git a/dir1/A b/dir1/A
  new file mode 100644
  --- /dev/null
  +++ b/dir1/A
  @@ -0,0 +1,1 @@
  +A1
  diff --git a/dir1/C b/dir1/C
  new file mode 100644
  --- /dev/null
  +++ b/dir1/C
  @@ -0,0 +1,1 @@
  +C10
  diff --git a/dir1/D b/dir1/D
  new file mode 100644
  --- /dev/null
  +++ b/dir1/D
  @@ -0,0 +1,2 @@
  +D0
  +D1
  diff --git a/dir2/A b/dir2/A
  new file mode 100644
  --- /dev/null
  +++ b/dir2/A
  @@ -0,0 +1,1 @@
  +A1
  diff --git a/dir2/C b/dir2/C
  new file mode 100644
  --- /dev/null
  +++ b/dir2/C
  @@ -0,0 +1,1 @@
  +C10
  diff --git a/dir2/D b/dir2/D
  new file mode 100644
  --- /dev/null
  +++ b/dir2/D
  @@ -0,0 +1,2 @@
  +D0
  +D1
  
Only changes the 1st commit:

  $ hg revert --config ui.origbackuppath=.hg/origbackups dir1/D
  $ echo A2 > dir1/A
  $ LOG=edenscm::hgext::dirsync=debug hg absorb
  showing changes for dir1/A
          @@ -0,1 +0,1 @@
  * -A1 (glob)
  * +A2 (glob)
  
  1 changeset affected
  * A (glob)
  apply changes (yn)?  y
  mirrored adding 'dir1/A' to 'dir2/A'
  mirrored changes in 'dir1/A' to 'dir2/A'
  DEBUG edenscm::hgext::dirsync: rewrite mirrored dir2/A
  1 of 1 chunk applied

  $ hg status
  $ hg log -p -T '{desc}\n' --config diff.git=1 dir2/A dir1/A
  A
  diff --git a/dir1/A b/dir1/A
  new file mode 100644
  --- /dev/null
  +++ b/dir1/A
  @@ -0,0 +1,1 @@
  +A2
  diff --git a/dir2/A b/dir2/A
  new file mode 100644
  --- /dev/null
  +++ b/dir2/A
  @@ -0,0 +1,1 @@
  +A2
  

  $ hg debugmutation -r .
   *  * absorb by test at 1970-01-01T00:00:00 from: (glob)
      * absorb by test at 1970-01-01T00:00:00 from: (glob)
      * (glob)
  
Changes the 1st commit but restores at the top:

  $ newrepo
  $ readconfig <<EOF
  > [dirsync]
  > sync1.1=dir1/
  > sync1.2=dir2/
  > EOF

  $ mkdir dir1
  $ echo 1 > dir1/A
  $ hg commit -m A1 -A dir1/A
  mirrored adding 'dir1/A' to 'dir2/A'

  $ echo 2 > dir1/A
  $ hg commit -m A2 dir1/A
  mirrored changes in 'dir1/A' to 'dir2/A'

  $ echo 3 > dir1/A
  $ hg commit -m A3 dir1/A
  mirrored changes in 'dir1/A' to 'dir2/A'

  $ HGEDITOR=cat hg absorb -e dir1/A
  apply changes (yn)?  y
  HG: editing dir1/A
  HG: "y" means the line to the right exists in the changeset to the top
  HG:
  HG: /---- * A1 (glob)
  HG: |/--- * A2 (glob)
  HG: ||/-- * A3 (glob)
  HG: |||
        y : 3
       y  : 2
      y   : 1
  nothing applied
  [1]

There is no "rewrite mirrored dir2/A" message:

  $ cat > editortext << EOF
  > HG: |||
  >       y : 3
  >      y  : 2
  >     y   : 3
  > EOF
  $ LOG='edenscm::hgext::dirsync=debug' HGEDITOR='cat editortext >' hg absorb --edit-lines -a dir1/A
  mirrored adding 'dir1/A' to 'dir2/A'
  1 of 1 chunk applied

and status is clean

  $ hg status
  ? editortext
  $ hg log -p -T '{desc}\n' --config diff.git=1 dir2/A
  A3
  diff --git a/dir2/A b/dir2/A
  --- a/dir2/A
  +++ b/dir2/A
  @@ -1,1 +1,1 @@
  -2
  +3
  
  A2
  diff --git a/dir2/A b/dir2/A
  --- a/dir2/A
  +++ b/dir2/A
  @@ -1,1 +1,1 @@
  -3
  +2
  
  A1
  diff --git a/dir2/A b/dir2/A
  new file mode 100644
  --- /dev/null
  +++ b/dir2/A
  @@ -0,0 +1,1 @@
  +3
  
