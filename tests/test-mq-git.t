# Test the plumbing of mq.git option
# Automatic upgrade itself is tested elsewhere.

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

  $ hg init repo-auto
  $ cd repo-auto

git=auto: regular patch creation:

  $ echo a > a
  $ hg add a
  $ hg qnew -d '0 0' -f adda

  $ cat .hg/patches/adda
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  # Date 0 0
  
  diff -r 000000000000 -r ef8dafc9fa4c a
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +a

git=auto: git patch creation with copy:

  $ hg cp a b
  $ hg qnew -d '0 0' -f copy

  $ cat .hg/patches/copy
  # HG changeset patch
  # Parent ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  # Date 0 0
  
  diff --git a/a b/b
  copy from a
  copy to b

git=auto: git patch when using --git:

  $ echo regular > regular
  $ hg add regular
  $ hg qnew -d '0 0' --git -f git

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 99586d5f048c399e20f81cee41fbb3809c0e735d
  # Date 0 0
  
  diff --git a/regular b/regular
  new file mode 100644
  --- /dev/null
  +++ b/regular
  @@ -0,0 +1,1 @@
  +regular

git=auto: regular patch after qrefresh without --git:

  $ hg qrefresh -d '0 0'

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 99586d5f048c399e20f81cee41fbb3809c0e735d
  # Date 0 0
  
  diff -r 99586d5f048c regular
  --- /dev/null
  +++ b/regular
  @@ -0,0 +1,1 @@
  +regular

  $ cd ..

  $ hg init repo-keep
  $ cd repo-keep
  $ echo '[mq]' > .hg/hgrc
  $ echo 'git = KEEP' >> .hg/hgrc

git=keep: git patch with --git:

  $ echo a > a
  $ hg add a
  $ hg qnew -d '0 0' -f --git git

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  # Date 0 0
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +a

git=keep: git patch after qrefresh without --git:

  $ echo a >> a
  $ hg qrefresh -d '0 0'

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  # Date 0 0
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  $ cd ..

  $ hg init repo-yes
  $ cd repo-yes
  $ echo '[mq]' > .hg/hgrc
  $ echo 'git = yes' >> .hg/hgrc

git=yes: git patch:

  $ echo a > a
  $ hg add a
  $ hg qnew -d '0 0' -f git

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  # Date 0 0
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +a

git=yes: git patch after qrefresh:

  $ echo a >> a
  $ hg qrefresh -d '0 0'

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent 0000000000000000000000000000000000000000
  # Date 0 0
  
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  $ cd ..

  $ hg init repo-no
  $ cd repo-no
  $ echo '[diff]' > .hg/hgrc
  $ echo 'git = True' >> .hg/hgrc
  $ echo '[mq]' > .hg/hgrc
  $ echo 'git = False' >> .hg/hgrc

git=no: regular patch with copy:

  $ echo a > a
  $ hg add a
  $ hg qnew -d '0 0' -f adda
  $ hg cp a b
  $ hg qnew -d '0 0' -f regular

  $ cat .hg/patches/regular
  # HG changeset patch
  # Parent ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  # Date 0 0
  
  diff -r ef8dafc9fa4c -r a70404f79ba3 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +a

git=no: regular patch after qrefresh with copy:

  $ hg cp a c
  $ hg qrefresh -d '0 0'

  $ cat .hg/patches/regular
  # HG changeset patch
  # Parent ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  # Date 0 0
  
  diff -r ef8dafc9fa4c b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +a
  diff -r ef8dafc9fa4c c
  --- /dev/null
  +++ b/c
  @@ -0,0 +1,1 @@
  +a

  $ cd ..

