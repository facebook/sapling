
  $ add()
  > {
  >     echo $2 >> $1
  > }
  $ hg init t
  $ cd t

set up a boring main branch

  $ add a a
  $ hg add a
  $ mkdir x
  $ add x/x x
  $ hg add x/x
  $ hg ci -m0
  $ add a m1
  $ hg ci -m1
  $ add a m2
  $ add x/y y1
  $ hg add x/y
  $ hg ci -m2
  $ cd ..
  $ show()
  > {
  >     echo "- $2: $1"
  >     hg st -C $1
  >     echo
  >     hg diff --git $1
  >     echo
  > }
  $ count=0

make a new branch and get diff/status output
$1 - first commit
$2 - second commit
$3 - working dir action
$4 - test description

  $ tb()
  > {
  >     hg clone t t2 ; cd t2
  >     hg co -q -C 0
  > 
  >     add a $count
  >     count=`expr $count + 1`
  >     hg ci -m "t0"
  >     $1
  >     hg ci -m "t1"
  >     $2
  >     hg ci -m "t2"
  >     $3
  > 
  >     echo "** $4 **"
  >     echo "** $1 / $2 / $3"
  >     show "" "working to parent"
  >     show "--rev 0" "working to root"
  >     show "--rev 2" "working to branch"
  >     show "--rev 0 --rev ." "root to parent"
  >     show "--rev . --rev 0" "parent to root"
  >     show "--rev 2 --rev ." "branch to parent"
  >     show "--rev . --rev 2" "parent to branch"
  >     echo
  >     cd ..
  >     rm -rf t2
  > }
  $ tb "add a a1" "add a a2" "hg mv a b" "rename in working dir"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** rename in working dir **
  ** add a a1 / add a a2 / hg mv a b
  - working to parent: 
  A b
    a
  R a
  
  diff --git a/a b/b
  rename from a
  rename to b
  
  - working to root: --rev 0
  A b
    a
  R a
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,4 @@
   a
  +0
  +a1
  +a2
  
  - working to branch: --rev 2
  A b
    a
  R a
  R x/y
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +0
  +a1
  +a2
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +0
  +a1
  +a2
  
  - parent to root: --rev . --rev 0
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -0
  -a1
  -a2
  
  - branch to parent: --rev 2 --rev .
  M a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +0
  +a1
  +a2
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  M a
  A x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,3 @@
   a
  -0
  -a1
  -a2
  +m1
  +m2
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "add a a1" "add a a2" "hg cp a b" "copy in working dir" 
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** copy in working dir **
  ** add a a1 / add a a2 / hg cp a b
  - working to parent: 
  A b
    a
  
  diff --git a/a b/b
  copy from a
  copy to b
  
  - working to root: --rev 0
  M a
  A b
    a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +1
  +a1
  +a2
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,4 @@
   a
  +1
  +a1
  +a2
  
  - working to branch: --rev 2
  M a
  A b
    a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +1
  +a1
  +a2
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +1
  +a1
  +a2
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +1
  +a1
  +a2
  
  - parent to root: --rev . --rev 0
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -1
  -a1
  -a2
  
  - branch to parent: --rev 2 --rev .
  M a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +1
  +a1
  +a2
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  M a
  A x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,3 @@
   a
  -1
  -a1
  -a2
  +m1
  +m2
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "hg mv a b" "add b b1" "add b w" "single rename"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** single rename **
  ** hg mv a b / add b b1 / add b w
  - working to parent: 
  M b
  
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,3 +1,4 @@
   a
   2
   b1
  +w
  
  - working to root: --rev 0
  A b
    a
  R a
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,4 @@
   a
  +2
  +b1
  +w
  
  - working to branch: --rev 2
  A b
    a
  R a
  R x/y
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,4 @@
   a
  -m1
  -m2
  +2
  +b1
  +w
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  A b
    a
  R a
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +2
  +b1
  
  - parent to root: --rev . --rev 0
  A a
    b
  R b
  
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,1 @@
   a
  -2
  -b1
  
  - branch to parent: --rev 2 --rev .
  A b
    a
  R a
  R x/y
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +2
  +b1
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  A a
    b
  A x/y
  R b
  
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,3 @@
   a
  -2
  -b1
  +m1
  +m2
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "hg cp a b" "add b b1" "add a w" "single copy"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** single copy **
  ** hg cp a b / add b b1 / add a w
  - working to parent: 
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
   3
  +w
  
  - working to root: --rev 0
  M a
  A b
    a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +3
  +w
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +3
  +b1
  
  - working to branch: --rev 2
  M a
  A b
    a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +3
  +w
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +3
  +b1
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  M a
  A b
    a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +3
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +3
  +b1
  
  - parent to root: --rev . --rev 0
  M a
  R b
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -3
  diff --git a/b b/b
  deleted file mode 100644
  --- a/b
  +++ /dev/null
  @@ -1,3 +0,0 @@
  -a
  -3
  -b1
  
  - branch to parent: --rev 2 --rev .
  M a
  A b
    a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +3
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +3
  +b1
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  M a
  A x/y
  R b
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
  -3
  +m1
  +m2
  diff --git a/b b/b
  deleted file mode 100644
  --- a/b
  +++ /dev/null
  @@ -1,3 +0,0 @@
  -a
  -3
  -b1
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "hg mv a b" "hg mv b c" "hg mv c d" "rename chain"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** rename chain **
  ** hg mv a b / hg mv b c / hg mv c d
  - working to parent: 
  A d
    c
  R c
  
  diff --git a/c b/d
  rename from c
  rename to d
  
  - working to root: --rev 0
  A d
    a
  R a
  
  diff --git a/a b/d
  rename from a
  rename to d
  --- a/a
  +++ b/d
  @@ -1,1 +1,2 @@
   a
  +4
  
  - working to branch: --rev 2
  A d
    a
  R a
  R x/y
  
  diff --git a/a b/d
  rename from a
  rename to d
  --- a/a
  +++ b/d
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +4
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  A c
    a
  R a
  
  diff --git a/a b/c
  rename from a
  rename to c
  --- a/a
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +4
  
  - parent to root: --rev . --rev 0
  A a
    c
  R c
  
  diff --git a/c b/a
  rename from c
  rename to a
  --- a/c
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -4
  
  - branch to parent: --rev 2 --rev .
  A c
    a
  R a
  R x/y
  
  diff --git a/a b/c
  rename from a
  rename to c
  --- a/a
  +++ b/c
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +4
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  A a
    c
  A x/y
  R c
  
  diff --git a/c b/a
  rename from c
  rename to a
  --- a/c
  +++ b/a
  @@ -1,2 +1,3 @@
   a
  -4
  +m1
  +m2
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "hg cp a b" "hg cp b c" "hg cp c d" "copy chain"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** copy chain **
  ** hg cp a b / hg cp b c / hg cp c d
  - working to parent: 
  A d
    c
  
  diff --git a/c b/d
  copy from c
  copy to d
  
  - working to root: --rev 0
  M a
  A b
    a
  A c
    a
  A d
    a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +5
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,2 @@
   a
  +5
  diff --git a/a b/c
  copy from a
  copy to c
  --- a/a
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +5
  diff --git a/a b/d
  copy from a
  copy to d
  --- a/a
  +++ b/d
  @@ -1,1 +1,2 @@
   a
  +5
  
  - working to branch: --rev 2
  M a
  A b
    a
  A c
    a
  A d
    a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/a b/c
  copy from a
  copy to c
  --- a/a
  +++ b/c
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/a b/d
  copy from a
  copy to d
  --- a/a
  +++ b/d
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  M a
  A b
    a
  A c
    a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +5
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,2 @@
   a
  +5
  diff --git a/a b/c
  copy from a
  copy to c
  --- a/a
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +5
  
  - parent to root: --rev . --rev 0
  M a
  R b
  R c
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -5
  diff --git a/b b/b
  deleted file mode 100644
  --- a/b
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -5
  diff --git a/c b/c
  deleted file mode 100644
  --- a/c
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -5
  
  - branch to parent: --rev 2 --rev .
  M a
  A b
    a
  A c
    a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/a b/b
  copy from a
  copy to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/a b/c
  copy from a
  copy to c
  --- a/a
  +++ b/c
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +5
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  M a
  A x/y
  R b
  R c
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
  -5
  +m1
  +m2
  diff --git a/b b/b
  deleted file mode 100644
  --- a/b
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -5
  diff --git a/c b/c
  deleted file mode 100644
  --- a/c
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -5
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "add a a1" "hg mv a b" "hg mv b a" "circular rename"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  ** circular rename **
  ** add a a1 / hg mv a b / hg mv b a
  - working to parent: 
  A a
    b
  R b
  
  diff --git a/b b/a
  rename from b
  rename to a
  
  - working to root: --rev 0
  M a
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +6
  +a1
  
  - working to branch: --rev 2
  M a
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +6
  +a1
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - root to parent: --rev 0 --rev .
  A b
    a
  R a
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +6
  +a1
  
  - parent to root: --rev . --rev 0
  A a
    b
  R b
  
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,1 @@
   a
  -6
  -a1
  
  - branch to parent: --rev 2 --rev .
  A b
    a
  R a
  R x/y
  
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,3 +1,3 @@
   a
  -m1
  -m2
  +6
  +a1
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  
  - parent to branch: --rev . --rev 2
  A a
    b
  A x/y
  R b
  
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,3 @@
   a
  -6
  -a1
  +m1
  +m2
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  
  $ tb "hg mv x y" "add y/x x1" "add y/x x2" "directory move"
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  created new head
  moving x/x to y/x
  ** directory move **
  ** hg mv x y / add y/x x1 / add y/x x2
  - working to parent: 
  M y/x
  
  diff --git a/y/x b/y/x
  --- a/y/x
  +++ b/y/x
  @@ -1,2 +1,3 @@
   x
   x1
  +x2
  
  - working to root: --rev 0
  M a
  A y/x
    x/x
  R x/x
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +7
  diff --git a/x/x b/y/x
  rename from x/x
  rename to y/x
  --- a/x/x
  +++ b/y/x
  @@ -1,1 +1,3 @@
   x
  +x1
  +x2
  
  - working to branch: --rev 2
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +7
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  diff --git a/x/x b/y/x
  rename from x/x
  rename to y/x
  --- a/x/x
  +++ b/y/x
  @@ -1,1 +1,3 @@
   x
  +x1
  +x2
  
  - root to parent: --rev 0 --rev .
  M a
  A y/x
    x/x
  R x/x
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +7
  diff --git a/x/x b/y/x
  rename from x/x
  rename to y/x
  --- a/x/x
  +++ b/y/x
  @@ -1,1 +1,2 @@
   x
  +x1
  
  - parent to root: --rev . --rev 0
  M a
  A x/x
    y/x
  R y/x
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -7
  diff --git a/y/x b/x/x
  rename from y/x
  rename to x/x
  --- a/y/x
  +++ b/x/x
  @@ -1,2 +1,1 @@
   x
  -x1
  
  - branch to parent: --rev 2 --rev .
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,2 @@
   a
  -m1
  -m2
  +7
  diff --git a/x/y b/x/y
  deleted file mode 100644
  --- a/x/y
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -y1
  diff --git a/x/x b/y/x
  rename from x/x
  rename to y/x
  --- a/x/x
  +++ b/y/x
  @@ -1,1 +1,2 @@
   x
  +x1
  
  - parent to branch: --rev . --rev 2
  M a
  A x/x
    y/x
  A x/y
  R y/x
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
  -7
  +m1
  +m2
  diff --git a/y/x b/x/x
  rename from y/x
  rename to x/x
  --- a/y/x
  +++ b/x/x
  @@ -1,2 +1,1 @@
   x
  -x1
  diff --git a/x/y b/x/y
  new file mode 100644
  --- /dev/null
  +++ b/x/y
  @@ -0,0 +1,1 @@
  +y1
  
  

Cannot implement unrelated branch with tb
testing copies with unrelated branch

  $ hg init unrelated
  $ cd unrelated
  $ add a a
  $ hg ci -Am adda
  adding a
  $ hg mv a b
  $ hg ci -m movea
  $ hg up -C null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ add a a
  $ hg ci -Am addunrelateda
  adding a
  created new head

unrelated branch diff

  $ hg diff --git -r 2 -r 1
  diff --git a/a b/a
  deleted file mode 100644
  --- a/a
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +a
  $ cd ..
