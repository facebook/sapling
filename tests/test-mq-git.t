# Test the plumbing of mq.git option
# Automatic upgrade itself is tested elsewhere.

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq =
  > [diff]
  > nodates = 1
  > EOF

  $ hg init repo-auto
  $ cd repo-auto

git=auto: regular patch creation:

  $ echo a > a
  $ hg add a
  $ hg qnew -d '0 0' -f adda

  $ cat .hg/patches/adda
  # HG changeset patch
  # Date 0 0
  # Parent  0000000000000000000000000000000000000000
  
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
  # Date 0 0
  # Parent  ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  
  diff --git a/a b/b
  copy from a
  copy to b

git=auto: git patch when using --git:

  $ echo regular > regular
  $ hg add regular
  $ hg qnew -d '0 0' --git -f git

  $ cat .hg/patches/git
  # HG changeset patch
  # Date 0 0
  # Parent  99586d5f048c399e20f81cee41fbb3809c0e735d
  
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
  # Date 0 0
  # Parent  99586d5f048c399e20f81cee41fbb3809c0e735d
  
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
  # Date 0 0
  # Parent  0000000000000000000000000000000000000000
  
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
  # Date 0 0
  # Parent  0000000000000000000000000000000000000000
  
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
  # Date 0 0
  # Parent  0000000000000000000000000000000000000000
  
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
  # Date 0 0
  # Parent  0000000000000000000000000000000000000000
  
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
  # Date 0 0
  # Parent  ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  
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
  # Date 0 0
  # Parent  ef8dafc9fa4caff80f6e243eb0171bcd60c455b4
  
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

Test how [diff] configuration influence and cause invalid or lossy patches:

  $ cat <<EOF >> .hg/hgrc
  > [mq]
  > git = AUTO
  > [diff]
  > nobinary = True
  > noprefix = True
  > showfunc = True
  > ignorews = True
  > ignorewsamount = True
  > ignoreblanklines = True
  > unified = 1
  > EOF

  $ echo ' a' > a
  $ hg qnew prepare -d '0 0'
  $ echo '  a' > a
  $ printf '\0' > b
  $ echo >> c
  $ hg qnew diff -d '0 0'

  $ cat .hg/patches/prepare
  # HG changeset patch
  # Date 0 0
  # Parent  cf0bfe72686a47d8d7d7b4529a3adb8b0b449a9f
  
  diff -r cf0bfe72686a -r fb9c4422b0f3 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  + a
  $ cat .hg/patches/diff
  # HG changeset patch
  # Date 0 0
  # Parent  fb9c4422b0f37dd576522dd9a3f99b825c177efe
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  - a
  +  a
  diff --git a/b b/b
  index 78981922613b2afb6025042ff6bd878ac1994e85..f76dd238ade08917e6712764a16a22005a50573d
  GIT binary patch
  literal 1
  Ic${MZ000310RR91
  
  diff --git a/c b/c
  --- a/c
  +++ b/c
  @@ -1,1 +1,2 @@
   a
  +

  $ cd ..

