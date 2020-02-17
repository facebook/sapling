#chg-compatible

  $ disable treemanifest
In this test, we want to test LFS bundle application. The test will cover all
combinations: LFS on/off; remotefilelog on/off.

To make it more interesting, the file revisions will contain hg filelog
metadata ('\1\n'). The bundle will have 1 file revision overlapping with the
destination repo.

#  rev      1          2         3
#  repo:    yes        yes       no
#  bundle:  no (base)  yes       yes (deltabase: 2 if possible)

It is interesting because rev 2 could have been stored as LFS in the repo, and
non-LFS in the bundle; or vice-versa.

Init:
  $ . helpers-usechg.sh

  $ enable lfs remotefilelog
  $ setconfig lfs.url=file://$TESTTMP/remote remotefilelog.cachepath=$TESTTMP/cache

Helper functions to create commits:

  $ commitxy() {
  > hg debugdrawdag "$@" <<'EOS'
  >  Y  # Y/X=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nE\nF (copied from Y)
  >  |  # Y/Y=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nG\nH (copied from X)
  >  X  # X/X=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nC\n
  >     # X/Y=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nD\n
  > EOS
  > }

  $ commitz() {
  > hg debugdrawdag "$@" <<'EOS'
  >  Z  # Z/X=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nI\n (copied from Y)
  >  |  # Z/Y=\1\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX\nJ\n (copied from X)
  >  |  # Z/Z=\1\nZ
  >  Y
  > EOS
  > }

  $ enablelfs() {
  >   cat >> .hg/hgrc <<EOF
  > [lfs]
  > threshold=1
  > EOF
  > }

  $ enableshallow() {
  >   echo remotefilelog >> .hg/requires
  > }

Generate bundles

  $ for i in shallow full; do
  >   for j in normal lfs; do
  >     NAME=src-$i-$j
  >     hg init $TESTTMP/$NAME
  >     cd $TESTTMP/$NAME
  >     [ $i = shallow ] && enableshallow
  >     [ $j = lfs ] && enablelfs
  >     commitxy
  >     commitz
  >     echo ---- Source repo: $i $j ----
  >     hg debugfilerevision -r 'all()'
  >     hg bundle -q --base X -r Y+Z $TESTTMP/$NAME.bundle
  >     SRCNAMES="$SRCNAMES $NAME"
  >   done
  > done
  ---- Source repo: shallow normal ----
  ed3e785005fc: X
   X: bin=0 lnk=0 flag=0 size=41 copied='' chain=e59d6c47cda0
   Y: bin=0 lnk=0 flag=0 size=41 copied='' chain=583fc1cb5a72
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=3 copied='' chain=0ad6e257ad34
  ---- Source repo: shallow lfs ----
  ed3e785005fc: X
   X: bin=0 lnk=0 flag=2000 size=41 copied='' chain=e59d6c47cda0
   Y: bin=0 lnk=0 flag=2000 size=41 copied='' chain=583fc1cb5a72
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Source repo: full normal ----
  ed3e785005fc: X
   X: bin=0 lnk=0 flag=0 size=45 copied='' chain=e59d6c47cda0
   Y: bin=0 lnk=0 flag=0 size=45 copied='' chain=583fc1cb5a72
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=583fc1cb5a72,88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39,5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=583fc1cb5a72,88c7303c7f80,78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=7 copied='' chain=0ad6e257ad34
  ---- Source repo: full lfs ----
  ed3e785005fc: X
   X: bin=0 lnk=0 flag=2000 size=41 copied='' chain=e59d6c47cda0
   Y: bin=0 lnk=0 flag=2000 size=41 copied='' chain=583fc1cb5a72
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34

Note: full normal repo has a wrong size=45 where it should be 41, see XXX note
in mercurial/filelog.py.

Prepare destination repos

  $ for i in shallow full; do
  >   for j in normal lfs; do
  >     NAME=dst-$i-$j
  >     hg init $TESTTMP/$NAME
  >     cd $TESTTMP/$NAME
  >     [ $i = shallow ] && enableshallow
  >     [ $j = lfs ] && enablelfs
  >     commitxy
  >     DSTNAMES="$DSTNAMES $NAME"
  >   done
  > done

Apply bundles

  $ cd $TESTTMP
  $ for i in $SRCNAMES; do
  >   for j in $DSTNAMES; do
  >     echo ---- Applying $i.bundle to $j ----
  >     cp -R $TESTTMP/$j $TESTTMP/tmp-$i-$j
  >     cd $TESTTMP/tmp-$i-$j
  >     hg unbundle $TESTTMP/$i.bundle -q 2>/dev/null || echo 'CRASHED!' && hg debugfilerev -r 'all()-X'
  >     if grep remotefilelog .hg/requires &>/dev/null; then :; else
  >       hg verify --quiet
  >     fi
  >   done
  > done
  ---- Applying src-shallow-normal.bundle to dst-shallow-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-normal.bundle to dst-shallow-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-normal.bundle to dst-full-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=583fc1cb5a72,88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=000000000000,5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=000000000000,78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=7 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-normal.bundle to dst-full-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=000000000000,5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=000000000000,78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=7 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-lfs.bundle to dst-shallow-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-lfs.bundle to dst-shallow-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-lfs.bundle to dst-full-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=583fc1cb5a72,88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-shallow-lfs.bundle to dst-full-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-normal.bundle to dst-shallow-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-normal.bundle to dst-shallow-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-normal.bundle to dst-full-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=583fc1cb5a72,88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39,5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=583fc1cb5a72,88c7303c7f80,78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=7 copied='' chain=0ad6e257ad34
  ---- Applying src-full-normal.bundle to dst-full-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=0 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=0 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=0 size=7 copied='' chain=0ad6e257ad34
  ---- Applying src-full-lfs.bundle to dst-shallow-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-lfs.bundle to dst-shallow-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-lfs.bundle to dst-full-normal ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=0 size=42 copied='Y' chain=e59d6c47cda0,c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=0 size=42 copied='X' chain=583fc1cb5a72,88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
  ---- Applying src-full-lfs.bundle to dst-full-lfs ----
  9f4445d5e0fc: Y
   X: bin=0 lnk=0 flag=2000 size=42 copied='Y' chain=c6fdd3c3ab39
   Y: bin=0 lnk=0 flag=2000 size=42 copied='X' chain=88c7303c7f80
  c73835eb729c: Z
   X: bin=0 lnk=0 flag=2000 size=41 copied='Y' chain=5322d1c20036
   Y: bin=0 lnk=0 flag=2000 size=41 copied='X' chain=78eb25c15608
   Z: bin=0 lnk=0 flag=2000 size=3 copied='' chain=0ad6e257ad34
