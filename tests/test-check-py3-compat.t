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
  hgext/convert/bzr.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/convert/convcmd.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/convert/subversion.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/convert/transport.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/fsmonitor/pywatchman/capabilities.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/fsmonitor/pywatchman/pybser.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/fsmonitor/watchmanclient.py: error importing: <ImportError> No module named 'pybser' (error at __init__.py:*)
  hgext/largefiles/basestore.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/lfcommands.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/lfutil.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/localstore.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/overrides.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/proto.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/remotestore.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/reposetup.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/storefactory.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/uisetup.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/largefiles/wirestore.py: error importing: <SyntaxError> cannot mix bytes and nonbytes literals (subversion.py, line 533) (error at convcmd.py:*)
  hgext/mq.py: error importing: <TypeError> __import__() argument 1 must be str, not bytes (error at extensions.py:*)
  mercurial/scmwindows.py: error importing: <ImportError> No module named 'winreg' (error at scmwindows.py:*)
  mercurial/win32.py: error importing: <ImportError> No module named 'msvcrt' (error at win32.py:*)
  mercurial/windows.py: error importing: <ImportError> No module named 'msvcrt' (error at windows.py:*)

#endif

#if py3exe py3pygments
  $ hg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
