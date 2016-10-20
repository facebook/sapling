  $ . "$TESTDIR/histedit-helpers.sh"

  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/fbhistedit.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > fbhistedit=$TESTTMP/fbhistedit.py
  > EOF

Enable obsolete

  $ cat > ${TESTTMP}/obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF

  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH
  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit

  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

exec & continue should not preserve hashes

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "this should be printed to stdout"
  > exec echo "this should be printed to stderr" >&2
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  this should be printed to stdout
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  this should be printed to stderr

  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
ensure we are properly executed in a shell
  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "foo" >/dev/null && exit 0
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

a failing command should drop us into the shell

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

continue should work

  $ hg histedit --continue

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

abort should work

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

  $ hg histedit --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

Multiple exec commands must work

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > exec exit 0
  > pick e860deea161a e
  > exec exit 0
  > exec exit 0
  > exec exit 0
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

amend should just work fine

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "NEW" > NEW && hg add NEW && hg commit --amend
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log --template '{node|short} {files} {desc}' --graph
  @  3cc63bf64c8d f f
  |
  o  8a564dc5ed88 NEW e e
  |
  o  055a42cdd887 d d
  |
  o  177f92b77385 c c
  |
  o  d2ae7f538514 b b
  |
  o  cb9a9f314b8b a a
  
amend should just work fine when sqldirstate is loaded but disabled
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "sqldirstate=$extpath/sqldirstate" >> .hg/hgrc
  $ echo "[sqldirstate]" >> .hg/hgrc
  $ echo "enabled=False" >> .hg/hgrc

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick 8a564dc5ed88 e
  > exec echo "NEW2" > NEW2 && hg add NEW2 && hg commit --amend
  > pick 3cc63bf64c8d f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log --template '{node|short} {files} {desc}' --graph
  @  f79e219dbfd3 f f
  |
  o  dc941479f5ce NEW NEW2 e e
  |
  o  055a42cdd887 d d
  |
  o  177f92b77385 c c
  |
  o  d2ae7f538514 b b
  |
  o  cb9a9f314b8b a a
  
