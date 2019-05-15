  $ newrepo
  $ mkdir -p a/b c/d/e c/f/g c/g
  $ cat > .gitignore << EOF
  > *.pyc
  > d/
  > g/
  > EOF
  $ echo '!a*.pyc' > a/.gitignore
  $ echo 'a1*.pyc' > a/b/.gitignore
  $ echo '!g/' > c/.gitignore
  $ echo 'g/' > c/f/.gitignore

  $ hg debugignore 1.pyc a/a1.pyc a/b/a10.pyc a/b/a2.pyc a/b/a2.py c/d/e/f c/d c/f/g/1/2 c/g/1/2 c/h/1
  1.pyc: Ignored by rule *.pyc in .gitignore
  a/a1.pyc: Whitelisted by rule !a*.pyc in a/.gitignore
  a/b/a10.pyc: Ignored by rule a1*.pyc in a/b/.gitignore
  a/b/a2.pyc: Whitelisted by rule !a*.pyc in a/.gitignore
  a/b/a2.py: Unspecified
  c/d/e/f: Ignored by rule d/ in .gitignore
  c/d: Ignored by rule d/ in .gitignore
  c/f/g/1/2: Ignored by rule g/ in c/f/.gitignore
  c/g/1/2: Unspecified
  c/h/1: Unspecified
  $ cat > $TESTTMP/globalignore << EOF
  > foo
  > EOF

  $ setconfig ui.ignore.1=$TESTTMP/globalignore

  $ hg debugignore foo
  foo: Ignored by rule foo in $TESTTMP/globalignore

