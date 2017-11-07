Test illegal name
-----------------

on commit:

  $ hg init hgname
  $ cd hgname
  $ mkdir sub
  $ hg init sub/.hg
  $ echo 'sub/.hg = sub/.hg' >> .hgsub
  $ hg ci -qAm 'add subrepo "sub/.hg"'
  abort: path 'sub/.hg' is inside nested repo 'sub'
  [255]

prepare tampered repo (including the commit above):

  $ hg import --bypass -qm 'add subrepo "sub/.hg"' - <<'EOF'
  > diff --git a/.hgsub b/.hgsub
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsub
  > @@ -0,0 +1,1 @@
  > +sub/.hg = sub/.hg
  > diff --git a/.hgsubstate b/.hgsubstate
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsubstate
  > @@ -0,0 +1,1 @@
  > +0000000000000000000000000000000000000000 sub/.hg
  > EOF
  $ cd ..

on clone (and update):

  $ hg clone -q hgname hgname2
  abort: path 'sub/.hg' is inside nested repo 'sub'
  [255]

Test direct symlink traversal
-----------------------------

#if symlink

on commit:

  $ mkdir hgsymdir
  $ hg init hgsymdir/root
  $ cd hgsymdir/root
  $ ln -s ../out
  $ hg ci -qAm 'add symlink "out"'
  $ hg init ../out
  $ echo 'out = out' >> .hgsub
  $ hg ci -qAm 'add subrepo "out"'
  abort: subrepo 'out' traverses symbolic link
  [255]

prepare tampered repo (including the commit above):

  $ hg import --bypass -qm 'add subrepo "out"' - <<'EOF'
  > diff --git a/.hgsub b/.hgsub
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsub
  > @@ -0,0 +1,1 @@
  > +out = out
  > diff --git a/.hgsubstate b/.hgsubstate
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsubstate
  > @@ -0,0 +1,1 @@
  > +0000000000000000000000000000000000000000 out
  > EOF
  $ cd ../..

on clone (and update):

  $ mkdir hgsymdir2
  $ hg clone -q hgsymdir/root hgsymdir2/root
  abort: subrepo 'out' traverses symbolic link
  [255]
  $ ls hgsymdir2
  root

#endif

Test indirect symlink traversal
-------------------------------

#if symlink

on commit:

  $ mkdir hgsymin
  $ hg init hgsymin/root
  $ cd hgsymin/root
  $ ln -s ../out
  $ hg ci -qAm 'add symlink "out"'
  $ mkdir ../out
  $ hg init ../out/sub
  $ echo 'out/sub = out/sub' >> .hgsub
  $ hg ci -qAm 'add subrepo "out/sub"'
  abort: path 'out/sub' traverses symbolic link 'out'
  [255]

prepare tampered repo (including the commit above):

  $ hg import --bypass -qm 'add subrepo "out/sub"' - <<'EOF'
  > diff --git a/.hgsub b/.hgsub
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsub
  > @@ -0,0 +1,1 @@
  > +out/sub = out/sub
  > diff --git a/.hgsubstate b/.hgsubstate
  > new file mode 100644
  > --- /dev/null
  > +++ b/.hgsubstate
  > @@ -0,0 +1,1 @@
  > +0000000000000000000000000000000000000000 out/sub
  > EOF
  $ cd ../..

on clone (and update):

  $ mkdir hgsymin2
  $ hg clone -q hgsymin/root hgsymin2/root
  abort: path 'out/sub' traverses symbolic link 'out'
  [255]
  $ ls hgsymin2
  root

#endif
