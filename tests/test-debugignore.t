#require no-windows

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
  1.pyc: ignored by rule *.pyc from .gitignore
  
  a/a1.pyc: ignored by rule *.pyc from .gitignore
  a/a1.pyc: unignored by rule !a*.pyc from a/.gitignore (overrides previous rules)
  
  a/b/a10.pyc: ignored by rule *.pyc from .gitignore
  a/b/a10.pyc: unignored by rule !a*.pyc from a/.gitignore (overrides previous rules)
  a/b/a10.pyc: ignored by rule a1*.pyc from a/b/.gitignore (overrides previous rules)
  
  a/b/a2.pyc: ignored by rule *.pyc from .gitignore
  a/b/a2.pyc: unignored by rule !a*.pyc from a/.gitignore (overrides previous rules)
  
  a/b/a2.py: not ignored
  
  c/d/e/f: ignored because c/d is ignored
  c/d: ignored by rule d/ from .gitignore
  
  c/d: ignored by rule d/ from .gitignore
  
  c/f/g/1/2: ignored because c/f/g is ignored
  c/f/g: ignored by rule g/ from .gitignore
  c/f/g: unignored by rule !g/ from c/.gitignore (overrides previous rules)
  c/f/g: ignored by rule g/ from c/f/.gitignore (overrides previous rules)
  
  c/g/1/2: not ignored
  
  c/h/1: not ignored
  
  $ cat > $TESTTMP/globalignore << EOF
  > foo
  > EOF

  $ setconfig ui.ignore.1=$TESTTMP/globalignore

  $ hg debugignore foo
  foo: ignored by rule foo from $TESTTMP/globalignore
  
