#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs python contrib/check-py3-compat.py
  contrib/python-zstandard/setup.py not using absolute_import
  contrib/python-zstandard/setup_zstd.py not using absolute_import
  contrib/python-zstandard/tests/common.py not using absolute_import
  contrib/python-zstandard/tests/test_compressor.py not using absolute_import
  contrib/python-zstandard/tests/test_data_structures.py not using absolute_import
  contrib/python-zstandard/tests/test_decompressor.py not using absolute_import
  contrib/python-zstandard/tests/test_estimate_sizes.py not using absolute_import
  contrib/python-zstandard/tests/test_module_attributes.py not using absolute_import
  contrib/python-zstandard/tests/test_roundtrip.py not using absolute_import
  contrib/python-zstandard/tests/test_train_dictionary.py not using absolute_import
  i18n/check-translation.py not using absolute_import
  setup.py not using absolute_import
  tests/test-demandimport.py not using absolute_import

#if py3exe
  $ hg files 'set:(**.py) - grep(pygments)' -X hgext/fsmonitor/pywatchman \
  > | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
  hgext/convert/transport.py: error importing: <ImportError> No module named 'svn.client' (error at transport.py:*)
  hgext/fsmonitor/state.py: error importing: <SyntaxError> from __future__ imports must occur at the beginning of the file (__init__.py, line 30) (error at watchmanclient.py:*)
  hgext/fsmonitor/watchmanclient.py: error importing: <SyntaxError> from __future__ imports must occur at the beginning of the file (__init__.py, line 30) (error at watchmanclient.py:*)
  mercurial/cffi/bdiff.py: error importing: <ImportError> No module named 'mercurial.cffi' (error at check-py3-compat.py:*)
  mercurial/cffi/mpatch.py: error importing: <ImportError> No module named 'mercurial.cffi' (error at check-py3-compat.py:*)
  mercurial/cffi/osutil.py: error importing: <ImportError> No module named 'mercurial.cffi' (error at check-py3-compat.py:*)
  mercurial/scmwindows.py: error importing: <ImportError> No module named 'msvcrt' (error at win32.py:*)
  mercurial/win32.py: error importing: <ImportError> No module named 'msvcrt' (error at win32.py:*)
  mercurial/windows.py: error importing: <ImportError> No module named 'msvcrt' (error at windows.py:*)

#endif

#if py3exe py3pygments
  $ hg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
