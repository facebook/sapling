#require test-repo pyflakes

  $ . "$TESTDIR/helpers-testrepo.sh"

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)

  $ cat > test.py <<EOF
  > print(undefinedname)
  > EOF
  $ pyflakes test.py 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  test.py:1: undefined name 'undefinedname'
  
  $ cd "`dirname "$TESTDIR"`"

  $ testrepohg locate '**.py' -I '.' \
  > -X hgext/fsmonitor/pywatchman \
  > -X mercurial/pycompat.py -X contrib/python-zstandard \
  > -X hg-git \
  > -X fb/facebook-hg-rpms \
  > -X fb/packaging \
  > 2>/dev/null \
  > | xargs pyflakes 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  setup.py:*: '*svn_swig_wrapper' imported but unused (glob)
  
