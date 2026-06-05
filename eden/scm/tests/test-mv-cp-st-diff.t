#chg-compatible
#require no-eden
  $ configure modernclient

  $ add()
  > {
  >     echo $2 >> $1
  > }
  $ newclientrepo t

set up a boring main branch

  $ add a a
  $ sl add a
  $ mkdir x
  $ add x/x x
  $ sl add x/x
  $ sl ci -m o0
  $ add a m1
  $ sl ci -m o1
  $ add a m2
  $ add x/y y1
  $ sl add x/y
  $ sl ci -m o2
  $ sl push -q --to book --create
  $ cd ..

  $ show()
  > {
  >     echo "# $2:"
  >     echo
  >     echo "% sl st -C $1"
  >     sl st -C $1
  >     echo
  >     echo "% sl diff --git $1"
  >     sl diff --git $1
  >     echo
  > }
  $ count=0

make a new branch and get diff/status output
$1 - first commit
$2 - second commit
$3 - working dir action

  $ tb()
  > {
  >     newclientrepo t2 t_server book
  >     sl co -q -C 'desc(o0)'
  > 
  >     echo % add a $count
  >     add a $count
  >     count=`expr $count + 1`
  >     echo % sl ci -m "t0"
  >     sl ci -m "t0"
  >     echo % $1
  >     $1
  >     echo % sl ci -m "t1"
  >     sl ci -m "t1"
  >     echo % $2
  >     $2
  >     echo % sl ci -m "t2"
  >     sl ci -m "t2"
  >     echo % $3
  >     $3
  >     echo
  >     show "" "working to parent"
  >     show "--rev desc(o0)" "working to root"
  >     show "--rev desc(o2)" "working to branch"
  >     show "--rev desc(o0) --rev ." "root to parent"
  >     show "--rev . --rev desc(o0)" "parent to root"
  >     show "--rev desc(o2) --rev ." "branch to parent"
  >     show "--rev . --rev desc(o2)" "parent to branch"
  >     echo
  >     cd ..
  >     rm -rf t2
  > }

rename in working dir

  $ tb "add a a1" "add a a2" "sl mv a b"
  % add a 0
  % sl ci -m t0
  % add a a1
  % sl ci -m t1
  % add a a2
  % sl ci -m t2
  % sl mv a b
  
  # working to parent:
  
  % sl st -C 
  A b
    a
  R a
  
  % sl diff --git 
  diff --git a/a b/b
  rename from a
  rename to b
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  A b
    a
  R a
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  A b
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  M a
  
  % sl diff --git --rev desc(o0) --rev .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +0
  +a1
  +a2
  
  # parent to root:
  
  % sl st -C --rev . --rev desc(o0)
  M a
  
  % sl diff --git --rev . --rev desc(o0)
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -0
  -a1
  -a2
  
  # branch to parent:
  
  % sl st -C --rev desc(o2) --rev .
  M a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  M a
  A x/y
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "add a a1" "add a a2" "sl cp a b"
  % add a 1
  % sl ci -m t0
  % add a a1
  % sl ci -m t1
  % add a a2
  % sl ci -m t2
  % sl cp a b
  
  # working to parent:
  
  % sl st -C 
  A b
    a
  
  % sl diff --git 
  diff --git a/a b/b
  copy from a
  copy to b
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  M a
  A b
    a
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  M a
  A b
    a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  M a
  
  % sl diff --git --rev desc(o0) --rev .
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,4 @@
   a
  +1
  +a1
  +a2
  
  # parent to root:
  
  % sl st -C --rev . --rev desc(o0)
  M a
  
  % sl diff --git --rev . --rev desc(o0)
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,4 +1,1 @@
   a
  -1
  -a1
  -a2
  
  # branch to parent:
  
  % sl st -C --rev desc(o2) --rev .
  M a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  M a
  A x/y
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "sl mv a b" "add b b1" "add b w"
  % add a 2
  % sl ci -m t0
  % sl mv a b
  % sl ci -m t1
  % add b b1
  % sl ci -m t2
  % add b w
  
  # working to parent:
  
  % sl st -C 
  M b
  
  % sl diff --git 
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,3 +1,4 @@
   a
   2
   b1
  +w
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  A b
    a
  R a
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  A b
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  A b
    a
  R a
  
  % sl diff --git --rev desc(o0) --rev .
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
  
  % sl st -C --rev . --rev desc(o0)
  A a
    b
  R b
  
  % sl diff --git --rev . --rev desc(o0)
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
  
  % sl st -C --rev desc(o2) --rev .
  A b
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  A a
    b
  A x/y
  R b
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "sl cp a b" "add b b1" "add a w"
  % add a 3
  % sl ci -m t0
  % sl cp a b
  % sl ci -m t1
  % add b b1
  % sl ci -m t2
  % add a w
  
  # working to parent:
  
  % sl st -C 
  M a
  
  % sl diff --git 
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
   3
  +w
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  M a
  A b
    a
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  M a
  A b
    a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  M a
  A b
    a
  
  % sl diff --git --rev desc(o0) --rev .
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
  
  % sl st -C --rev . --rev desc(o0)
  M a
  R b
  
  % sl diff --git --rev . --rev desc(o0)
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
  
  % sl st -C --rev desc(o2) --rev .
  M a
  A b
    a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  M a
  A x/y
  R b
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "sl mv a b" "sl mv b c" "sl mv c d"
  % add a 4
  % sl ci -m t0
  % sl mv a b
  % sl ci -m t1
  % sl mv b c
  % sl ci -m t2
  % sl mv c d
  
  # working to parent:
  
  % sl st -C 
  A d
    c
  R c
  
  % sl diff --git 
  diff --git a/c b/d
  rename from c
  rename to d
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  A d
    a
  R a
  
  % sl diff --git --rev desc(o0)
  diff --git a/a b/d
  rename from a
  rename to d
  --- a/a
  +++ b/d
  @@ -1,1 +1,2 @@
   a
  +4
  
  # working to branch:
  
  % sl st -C --rev desc(o2)
  A d
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  A c
    a
  R a
  
  % sl diff --git --rev desc(o0) --rev .
  diff --git a/a b/c
  rename from a
  rename to c
  --- a/a
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +4
  
  # parent to root:
  
  % sl st -C --rev . --rev desc(o0)
  A a
    c
  R c
  
  % sl diff --git --rev . --rev desc(o0)
  diff --git a/c b/a
  rename from c
  rename to a
  --- a/c
  +++ b/a
  @@ -1,2 +1,1 @@
   a
  -4
  
  # branch to parent:
  
  % sl st -C --rev desc(o2) --rev .
  A c
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  A a
    c
  A x/y
  R c
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "sl cp a b" "sl cp b c" "sl cp c d"
  % add a 5
  % sl ci -m t0
  % sl cp a b
  % sl ci -m t1
  % sl cp b c
  % sl ci -m t2
  % sl cp c d
  
  # working to parent:
  
  % sl st -C 
  A d
    c
  
  % sl diff --git 
  diff --git a/c b/d
  copy from c
  copy to d
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  M a
  A b
    a
  A c
    a
  A d
    a
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  M a
  A b
    a
  A c
    a
  A d
    a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  M a
  A b
    a
  A c
    a
  
  % sl diff --git --rev desc(o0) --rev .
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
  
  % sl st -C --rev . --rev desc(o0)
  M a
  R b
  R c
  
  % sl diff --git --rev . --rev desc(o0)
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
  
  % sl st -C --rev desc(o2) --rev .
  M a
  A b
    a
  A c
    a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  M a
  A x/y
  R b
  R c
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "add a a1" "sl mv a b" "sl mv b a"
  % add a 6
  % sl ci -m t0
  % add a a1
  % sl ci -m t1
  % sl mv a b
  % sl ci -m t2
  % sl mv b a
  
  # working to parent:
  
  % sl st -C 
  A a
    b
  R b
  
  % sl diff --git 
  diff --git a/b b/a
  rename from b
  rename to a
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  M a
  
  % sl diff --git --rev desc(o0)
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +6
  +a1
  
  # working to branch:
  
  % sl st -C --rev desc(o2)
  M a
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  A b
    a
  R a
  
  % sl diff --git --rev desc(o0) --rev .
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
  
  % sl st -C --rev . --rev desc(o0)
  A a
    b
  R b
  
  % sl diff --git --rev . --rev desc(o0)
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
  
  % sl st -C --rev desc(o2) --rev .
  A b
    a
  R a
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  A a
    b
  A x/y
  R b
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ tb "sl mv x y" "add y/x x1" "add y/x x2"
  % add a 7
  % sl ci -m t0
  % sl mv x y
  moving x/x to y/x
  % sl ci -m t1
  % add y/x x1
  % sl ci -m t2
  % add y/x x2
  
  # working to parent:
  
  % sl st -C 
  M y/x
  
  % sl diff --git 
  diff --git a/y/x b/y/x
  --- a/y/x
  +++ b/y/x
  @@ -1,2 +1,3 @@
   x
   x1
  +x2
  
  # working to root:
  
  % sl st -C --rev desc(o0)
  M a
  A y/x
    x/x
  R x/x
  
  % sl diff --git --rev desc(o0)
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
  
  % sl st -C --rev desc(o2)
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  % sl diff --git --rev desc(o2)
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
  
  % sl st -C --rev desc(o0) --rev .
  M a
  A y/x
    x/x
  R x/x
  
  % sl diff --git --rev desc(o0) --rev .
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
  
  % sl st -C --rev . --rev desc(o0)
  M a
  A x/x
    y/x
  R y/x
  
  % sl diff --git --rev . --rev desc(o0)
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
  
  % sl st -C --rev desc(o2) --rev .
  M a
  A y/x
    x/x
  R x/x
  R x/y
  
  % sl diff --git --rev desc(o2) --rev .
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
  
  % sl st -C --rev . --rev desc(o2)
  M a
  A x/x
    y/x
  A x/y
  R y/x
  
  % sl diff --git --rev . --rev desc(o2)
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

  $ newclientrepo unrelated
  $ echo a >> a
  $ sl ci -Am adda
  adding a
  $ sl mv a b
  $ sl ci -m movea
  $ sl up -C null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a >> a
  $ sl ci -Am addunrelateda
  adding a

unrelated branch diff

  $ sl diff --git -r 'desc(addunrelateda)' -r 'desc(movea)'
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

  $ newclientrepo diffstop
  $ echo > f
  $ sl ci -qAmf
  $ sl mv f g
  $ sl ci -m'f->g'
  $ sl up -qr .^
  $ touch x
  $ sl ci -qAmx
  $ echo f > f
  $ sl ci -qmf=f
  $ sl merge -q
  $ sl ci -mmerge
  $ sl log -G --template '{desc}'
  @    merge
  ├─╮
  │ o  f=f
  │ │
  │ o  x
  │ │
  o │  f->g
  ├─╯
  o  f
  
  $ sl diff --git -r 'desc("f=f")'
  diff --git a/f b/g
  rename from f
  rename to g
  $ cd ..

Additional tricky linkrev case
------------------------------

If the first file revision after the diff base has a linkrev pointing to a
changeset on another branch with a revision lower that the diff base, we can
jump past the copy detection limit and fail to detect the rename.

  $ newclientrepo diffstoplinkrev

  $ touch f
  $ sl ci -Aqm 'empty f'

Make a simple change

  $ echo change > f
  $ sl ci -m 'change f'

Make a second branch, we use a named branch to create a simple commit
that does not touch f.

  $ sl up -qr 'desc(empty)'
  $ sl ci -Aqm dev --config ui.allowemptycommit=1

Graft the initial change, as f was untouched, we reuse the same entry and the
linkrev point to the older branch.

  $ sl graft -q 'desc(change)'

Make a rename because we want to track renames. It is also important that the
faulty linkrev is not the "start" commit to ensure the linkrev will be used.

  $ sl mv f renamed
  $ sl ci -m renamed

  $ sl log -G -T '{desc}'
  @  renamed
  │
  o  change f
  │
  o  dev
  │
  │ o  change f
  ├─╯
  o  empty f
  

The copy tracking should still reach rev 2 (branch creation).
accessing the parent of 4 (renamed) should not jump use to revision 1.

  $ sl diff --git -r 'desc(dev)' -r .
  diff --git a/f b/renamed
  rename from f
  rename to renamed
  --- a/f
  +++ b/renamed
  @@ -0,0 +1,1 @@
  +change

  $ cd ..
