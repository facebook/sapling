  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[mq]" >> $HGRCPATH
  $ echo "git=keep" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

init:

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg qnew -f p1
  $ echo b >> a
  $ hg qnew -f p2
  $ echo c >> a
  $ hg qnew -f p3

Fold in the middle of the queue:

  $ hg qpop p1
  popping p3
  popping p2
  now at: p1

  $ hg qdiff
  diff -r 07f494440405 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
   a
  +a

  $ hg qfold p2
  $ grep git .hg/patches/p1 && echo 'git patch found!'
  [1]

  $ hg qser
  p1
  p3

  $ hg qdiff
  diff -r 07f494440405 a
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +a
  +b

Fold with local changes:

  $ echo d >> a
  $ hg qfold p3
  abort: local changes found, refresh first
  [255]

  $ hg diff -c .
  diff -r 07f494440405 -r ???????????? a (glob)
  --- a/a
  +++ b/a
  @@ -1,1 +1,3 @@
   a
  +a
  +b

  $ hg revert -a --no-backup
  reverting a

Fold git patch into a regular patch, expect git patch:

  $ echo a >> a
  $ hg qnew -f regular
  $ hg cp a aa
  $ hg qnew --git -f git

  $ hg qpop
  popping git
  now at: regular

  $ hg qfold git

  $ cat .hg/patches/regular
  # HG changeset patch
  # Parent ???????????????????????????????????????? (glob)
  
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,4 @@
   a
   a
   b
  +a
  diff --git a/a b/aa
  copy from a
  copy to aa
  --- a/a
  +++ b/aa
  @@ -1,3 +1,4 @@
   a
   a
   b
  +a

  $ hg qpop
  popping regular
  now at: p1

  $ hg qdel regular

Fold regular patch into a git patch, expect git patch:

  $ hg cp a aa
  $ hg qnew --git -f git
  $ echo b >> aa
  $ hg qnew -f regular

  $ hg qpop
  popping regular
  now at: git

  $ hg qfold regular

  $ cat .hg/patches/git
  # HG changeset patch
  # Parent ???????????????????????????????????????? (glob)
  
  diff --git a/a b/aa
  copy from a
  copy to aa
  --- a/a
  +++ b/aa
  @@ -1,3 +1,4 @@
   a
   a
   b
  +b

Test saving last-message.txt:

  $ hg qrefresh -m "original message"

  $ cat > $TESTTMP/commitfailure.py <<EOF
  > from mercurial import util
  > def reposetup(ui, repo):
  >     class commitfailure(repo.__class__):
  >         def commit(self, *args, **kwargs):
  >             raise util.Abort('emulating unexpected abort')
  >     repo.__class__ = commitfailure
  > EOF

  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > commitfailure = $TESTTMP/commitfailure.py
  > EOF

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > (echo; echo "test saving last-message.txt") >> \$1
  > EOF

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg qfold -e p3
  ==== before editing
  original message====
  refresh interrupted while patch was popped! (revert --all, qpush to recover)
  abort: emulating unexpected abort
  [255]
  $ cat .hg/last-message.txt
  original message
  test saving last-message.txt

  $ cd ..

