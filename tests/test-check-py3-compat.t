#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs python contrib/check-py3-compat.py
  hgext/fsmonitor/pywatchman/__init__.py not using absolute_import
  hgext/fsmonitor/pywatchman/__init__.py requires print_function
  hgext/fsmonitor/pywatchman/capabilities.py not using absolute_import
  hgext/fsmonitor/pywatchman/pybser.py not using absolute_import
  i18n/check-translation.py not using absolute_import
  setup.py not using absolute_import
  tests/test-demandimport.py not using absolute_import

#if py3exe
  $ hg files 'set:(**.py) - grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
  hgext/convert/transport.py: error importing: <ImportError> No module named 'svn.client' (error at transport.py:*)
  hgext/fsmonitor/pywatchman/capabilities.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/fsmonitor/pywatchman/pybser.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/fsmonitor/watchmanclient.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/mq.py: error importing: <TypeError> __import__() argument 1 must be str, not bytes (error at extensions.py:*)
  mercurial/scmwindows.py: error importing: <ImportError> No module named 'winreg' (error at scmwindows.py:*)
  mercurial/statprof.py: error importing: <TypeError> __slots__ items must be strings, not 'bytes' (error at statprof.py:*)
  mercurial/win32.py: error importing: <ImportError> No module named 'msvcrt' (error at win32.py:*)
  mercurial/windows.py: error importing: <ImportError> No module named 'msvcrt' (error at windows.py:*)

#endif

#if py3exe py3pygments
  $ hg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
