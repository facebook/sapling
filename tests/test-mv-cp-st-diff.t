
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
  >     echo "# $2:"
  >     echo
  >     echo "% hg st -C $1"
  >     hg st -C $1
  >     echo
  >     echo "% hg diff --git $1"
  >     hg diff --git $1
  >     echo
  > }
  $ count=0

make a new branch and get diff/status output
$1 - first commit
$2 - second commit
$3 - working dir action

  $ tb()
  > {
  >     hg clone -q t t2 ; cd t2
  >     hg co -q -C 0
  > 
  >     echo % add a $count
  >     add a $count
  >     count=`expr $count + 1`
  >     echo % hg ci -m "t0"
  >     hg ci -m "t0"
  >     echo % $1
  >     $1
  >     echo % hg ci -m "t1"
  >     hg ci -m "t1"
  >     echo % $2
  >     $2
  >     echo % hg ci -m "t2"
  >     hg ci -m "t2"
  >     echo % $3
  >     $3
  >     echo
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

rename in working dir

  $ tb "add a a1" "add a a2" "hg mv a b"
  % add a 0
  % hg ci -m t0
  created new head
  % add a a1
  % hg ci -m t1
  % add a a2
  % hg ci -m t2
  % hg mv a b
  
  # working to parent:
  
  % hg st -C 
  A b
    a
  R a
  
  % hg diff --git 
  diff --git a/a b/b
  rename from a
  rename to b
  
  # working to root:
  
  % hg st -C --rev 0
  A b
    a
  R a
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  A b
    a
  R a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  M a
  
  % hg diff --git --rev 0 --rev .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +0
  +a1
  +a2
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  M a
  
  % hg diff --git --rev . --rev 0
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -0
  -a1
  -a2
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  M a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  M a
  A x/y
  
  % hg diff --git --rev . --rev 2
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
  
  
copy in working dir

  $ tb "add a a1" "add a a2" "hg cp a b"
  % add a 1
  % hg ci -m t0
  created new head
  % add a a1
  % hg ci -m t1
  % add a a2
  % hg ci -m t2
  % hg cp a b
  
  # working to parent:
  
  % hg st -C 
  A b
    a
  
  % hg diff --git 
  diff --git a/a b/b
  copy from a
  copy to b
  
  # working to root:
  
  % hg st -C --rev 0
  M a
  A b
    a
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  M a
  A b
    a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  M a
  
  % hg diff --git --rev 0 --rev .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +1
  +a1
  +a2
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  M a
  
  % hg diff --git --rev . --rev 0
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -1
  -a1
  -a2
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  M a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  M a
  A x/y
  
  % hg diff --git --rev . --rev 2
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
  
  
single rename

  $ tb "hg mv a b" "add b b1" "add b w"
  % add a 2
  % hg ci -m t0
  created new head
  % hg mv a b
  % hg ci -m t1
  % add b b1
  % hg ci -m t2
  % add b w
  
  # working to parent:
  
  % hg st -C 
  M b
  
  % hg diff --git 
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,3 +1,4 @@
   a
   2
   b1
  +w
  
  # working to root:
  
  % hg st -C --rev 0
  A b
    a
  R a
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  A b
    a
  R a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  A b
    a
  R a
  
  % hg diff --git --rev 0 --rev .
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +2
  +b1
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  A a
    b
  R b
  
  % hg diff --git --rev . --rev 0
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,1 @@
   a
  -2
  -b1
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  A b
    a
  R a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  A a
    b
  A x/y
  R b
  
  % hg diff --git --rev . --rev 2
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
  
  
single copy

  $ tb "hg cp a b" "add b b1" "add a w"
  % add a 3
  % hg ci -m t0
  created new head
  % hg cp a b
  % hg ci -m t1
  % add b b1
  % hg ci -m t2
  % add a w
  
  # working to parent:
  
  % hg st -C 
  M a
  
  % hg diff --git 
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
   3
  +w
  
  # working to root:
  
  % hg st -C --rev 0
  M a
  A b
    a
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  M a
  A b
    a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  M a
  A b
    a
  
  % hg diff --git --rev 0 --rev .
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
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  M a
  R b
  
  % hg diff --git --rev . --rev 0
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
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  M a
  A b
    a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  M a
  A x/y
  R b
  
  % hg diff --git --rev . --rev 2
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
  
  
rename chain

  $ tb "hg mv a b" "hg mv b c" "hg mv c d"
  % add a 4
  % hg ci -m t0
  created new head
  % hg mv a b
  % hg ci -m t1
  % hg mv b c
  % hg ci -m t2
  % hg mv c d
  
  # working to parent:
  
  % hg st -C 
  A d
    c
  R c
  
  % hg diff --git 
  diff --git a/c b/d
  rename from c
  rename to d
  
  # working to root:
  
  % hg st -C --rev 0
  A d
    a
  R a
  
  % hg diff --git --rev 0
  diff --git a/a b/d
  rename from a
  rename to d
  --- a/a
  +++ b/d
  @@ -1,1 +1,2 @@
   a
  +4
  
  # working to branch:
  
  % hg st -C --rev 2
  A d
    a
  R a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  A c
    a
  R a
  
  % hg diff --git --rev 0 --rev .
  diff --git a/a b/c
  rename from a
  rename to c
  --- a/a
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +4
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  A a
    c
  R c
  
  % hg diff --git --rev . --rev 0
  diff --git a/c b/a
  rename from c
  rename to a
  --- a/c
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -4
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  A c
    a
  R a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  A a
    c
  A x/y
  R c
  
  % hg diff --git --rev . --rev 2
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
  
  
copy chain

  $ tb "hg cp a b" "hg cp b c" "hg cp c d"
  % add a 5
  % hg ci -m t0
  created new head
  % hg cp a b
  % hg ci -m t1
  % hg cp b c
  % hg ci -m t2
  % hg cp c d
  
  # working to parent:
  
  % hg st -C 
  A d
    c
  
  % hg diff --git 
  diff --git a/c b/d
  copy from c
  copy to d
  
  # working to root:
  
  % hg st -C --rev 0
  M a
  A b
    a
  A c
    a
  A d
    a
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  M a
  A b
    a
  A c
    a
  A d
    a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  M a
  A b
    a
  A c
    a
  
  % hg diff --git --rev 0 --rev .
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
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  M a
  R b
  R c
  
  % hg diff --git --rev . --rev 0
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
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  M a
  A b
    a
  A c
    a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  M a
  A x/y
  R b
  R c
  
  % hg diff --git --rev . --rev 2
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
  
  
circular rename

  $ tb "add a a1" "hg mv a b" "hg mv b a"
  % add a 6
  % hg ci -m t0
  created new head
  % add a a1
  % hg ci -m t1
  % hg mv a b
  % hg ci -m t2
  % hg mv b a
  
  # working to parent:
  
  % hg st -C 
  A a
    b
  R b
  
  % hg diff --git 
  diff --git a/b b/a
  rename from b
  rename to a
  
  # working to root:
  
  % hg st -C --rev 0
  M a
  
  % hg diff --git --rev 0
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +6
  +a1
  
  # working to branch:
  
  % hg st -C --rev 2
  M a
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  A b
    a
  R a
  
  % hg diff --git --rev 0 --rev .
  diff --git a/a b/b
  rename from a
  rename to b
  --- a/a
  +++ b/b
  @@ -1,1 +1,3 @@
   a
  +6
  +a1
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  A a
    b
  R b
  
  % hg diff --git --rev . --rev 0
  diff --git a/b b/a
  rename from b
  rename to a
  --- a/b
  +++ b/a
  @@ -1,3 +1,1 @@
   a
  -6
  -a1
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  A b
    a
  R a
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  A a
    b
  A x/y
  R b
  
  % hg diff --git --rev . --rev 2
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
  
  
directory move

  $ tb "hg mv x y" "add y/x x1" "add y/x x2"
  % add a 7
  % hg ci -m t0
  created new head
  % hg mv x y
  moving x/x to y/x (glob)
  % hg ci -m t1
  % add y/x x1
  % hg ci -m t2
  % add y/x x2
  
  # working to parent:
  
  % hg st -C 
  M y/x
  
  % hg diff --git 
  diff --git a/y/x b/y/x
  --- a/y/x
  +++ b/y/x
  @@ -1,2 +1,3 @@
   x
   x1
  +x2
  
  # working to root:
  
  % hg st -C --rev 0
  M a
  A y/x
    x/x
  R x/x
  
  % hg diff --git --rev 0
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
  
  # working to branch:
  
  % hg st -C --rev 2
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  % hg diff --git --rev 2
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
  
  # root to parent:
  
  % hg st -C --rev 0 --rev .
  M a
  A y/x
    x/x
  R x/x
  
  % hg diff --git --rev 0 --rev .
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
  
  # parent to root:
  
  % hg st -C --rev . --rev 0
  M a
  A x/x
    y/x
  R y/x
  
  % hg diff --git --rev . --rev 0
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
  
  # branch to parent:
  
  % hg st -C --rev 2 --rev .
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  % hg diff --git --rev 2 --rev .
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
  
  # parent to branch:
  
  % hg st -C --rev . --rev 2
  M a
  A x/x
    y/x
  A x/y
  R y/x
  
  % hg diff --git --rev . --rev 2
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
  $ echo a >> a
  $ hg ci -Am adda
  adding a
  $ hg mv a b
  $ hg ci -m movea
  $ hg up -C null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
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


test for case where we didn't look sufficiently far back to find rename ancestor

  $ hg init diffstop
  $ cd diffstop
  $ echo > f
  $ hg ci -qAmf
  $ hg mv f g
  $ hg ci -m'f->g'
  $ hg up -qr0
  $ touch x
  $ hg ci -qAmx
  $ echo f > f
  $ hg ci -qmf=f
  $ hg merge -q
  $ hg ci -mmerge
  $ hg log -G --template '{rev}  {desc}'
  @    4  merge
  |\
  | o  3  f=f
  | |
  | o  2  x
  | |
  o |  1  f->g
  |/
  o  0  f
  
  $ hg diff --git -r 2
  diff --git a/f b/g
  rename from f
  rename to g
  --- a/f
  +++ b/g
  @@ -1,1 +1,1 @@
  -
  +f
  $ cd ..

Additional tricky linkrev case
------------------------------

If the first file revision after the diff base has a linkrev pointing to a
changeset on another branch with a revision lower that the diff base, we can
jump past the copy detection limit and fail to detect the rename.

  $ hg init diffstoplinkrev
  $ cd diffstoplinkrev

  $ touch f
  $ hg ci -Aqm 'empty f'

Make a simple change

  $ echo change > f
  $ hg ci -m 'change f'

Make a second branch, we use a named branch to create a simple commit
that does not touch f.

  $ hg up -qr 'desc(empty)'
  $ hg branch -q dev
  $ hg ci -Aqm dev

Graft the initial change, as f was untouched, we reuse the same entry and the
linkrev point to the older branch.

  $ hg graft -q 'desc(change)'

Make a rename because we want to track renames. It is also important that the
faulty linkrev is not the "start" commit to ensure the linkrev will be used.

  $ hg mv f renamed
  $ hg ci -m renamed

  $ hg log -G -T '{rev} {desc}'
  @  4 renamed
  |
  o  3 change f
  |
  o  2 dev
  |
  | o  1 change f
  |/
  o  0 empty f
  

The copy tracking should still reach rev 2 (branch creation).
accessing the parent of 4 (renamed) should not jump use to revision 1.

  $ hg diff --git -r 'desc(dev)' -r .
  diff --git a/f b/renamed
  rename from f
  rename to renamed
  --- a/f
  +++ b/renamed
  @@ -0,0 +1,1 @@
  +change

  $ cd ..
