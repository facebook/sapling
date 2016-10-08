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
  hgext/convert/bzr.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/convcmd.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/cvs.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/darcs.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/filemap.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/git.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/gnuarch.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/hg.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/monotone.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/p4.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/subversion.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *)
  hgext/convert/transport.py: error importing module: <ImportError> No module named 'svn.client' (line *)
  hgext/fsmonitor/watchmanclient.py: error importing module: <SystemError> Parent module 'hgext.fsmonitor' not loaded, cannot perform relative import (line *)
  hgext/journal.py: error importing module: <SystemError> Parent module 'hgext' not loaded, cannot perform relative import (line *)
  hgext/largefiles/basestore.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/lfcommands.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/localstore.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/overrides.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/proto.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/remotestore.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/reposetup.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/storefactory.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/uisetup.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/largefiles/wirestore.py: error importing module: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (line *)
  hgext/mq.py: error importing: <TypeError> startswith first arg must be str or a tuple of str, not bytes (error at extensions.py:*)
  hgext/rebase.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  hgext/record.py: error importing module: <KeyError> '^commit|ci' (line *)
  hgext/shelve.py: error importing module: <SystemError> Parent module 'hgext' not loaded, cannot perform relative import (line *)
  hgext/transplant.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  mercurial/encoding.py: error importing module: <TypeError> bytes expected, not str (line *)
  mercurial/fileset.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  mercurial/i18n.py: error importing module: <TypeError> bytes expected, not str (line *)
  mercurial/revset.py: error importing module: <AttributeError> 'dict' object has no attribute 'iteritems' (line *)
  mercurial/scmwindows.py: error importing module: <ImportError> No module named 'winreg' (line *)
  mercurial/store.py: error importing module: <TypeError> Can't convert 'bytes' object to str implicitly (line *)
  mercurial/win32.py: error importing module: <ImportError> No module named 'msvcrt' (line *)
  mercurial/windows.py: error importing module: <ImportError> No module named 'msvcrt' (line *)

#endif

#if py3exe py3pygments
  $ hg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
