#require test-repo pyflakes

  $ . "$TESTDIR/helpers-testrepo.sh"

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)

  $ PYFLAKES=${HGTEST_PYFLAKES_PATH:-pyflakes}
  $ cat > test.py <<EOF
  > print(undefinedname)
  > EOF
  $ "$PYFLAKES" test.py 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  test.py:1: undefined name 'undefinedname'
  
  $ cd "`dirname "$TESTDIR"`"

  $ testrepohg locate '**.py' -I '.' \
  > -X hgext/fsmonitor/pywatchman \
  > -X mercurial/pycompat.py -X contrib/python-zstandard \
  > -X hg-git \
  > -X fb \
  > 2>/dev/null \
  > | xargs "$PYFLAKES" 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  
