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
  hgext/convert/bzr.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at bzr.py:*)
  hgext/convert/convcmd.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at convcmd.py:*)
  hgext/convert/cvs.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at cvs.py:*)
  hgext/convert/darcs.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at darcs.py:*)
  hgext/convert/filemap.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at filemap.py:*)
  hgext/convert/git.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at git.py:*)
  hgext/convert/gnuarch.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at gnuarch.py:*)
  hgext/convert/hg.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at hg.py:*)
  hgext/convert/monotone.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at monotone.py:*)
  hgext/convert/p4.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at p4.py:*)
  hgext/convert/subversion.py: error importing: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (error at subversion.py:*)
  hgext/convert/transport.py: error importing: <ImportError> No module named 'svn.client' (error at transport.py:*)
  hgext/fsmonitor/watchmanclient.py: error importing: <SystemError> Parent module 'hgext.fsmonitor' not loaded, cannot perform relative import (error at watchmanclient.py:*)
  hgext/journal.py: error importing: <SystemError> Parent module 'hgext' not loaded, cannot perform relative import (error at journal.py:*)
  hgext/largefiles/basestore.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at basestore.py:*)
  hgext/largefiles/lfcommands.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at lfcommands.py:*)
  hgext/largefiles/localstore.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at localstore.py:*)
  hgext/largefiles/overrides.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at overrides.py:*)
  hgext/largefiles/proto.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at proto.py:*)
  hgext/largefiles/remotestore.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at remotestore.py:*)
  hgext/largefiles/reposetup.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at reposetup.py:*)
  hgext/largefiles/storefactory.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at storefactory.py:*)
  hgext/largefiles/uisetup.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at uisetup.py:*)
  hgext/largefiles/wirestore.py: error importing: <SystemError> Parent module 'hgext.largefiles' not loaded, cannot perform relative import (error at wirestore.py:*)
  hgext/mq.py: error importing: <TypeError> startswith first arg must be str or a tuple of str, not bytes (error at extensions.py:*)
  hgext/rebase.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  hgext/record.py: error importing: <KeyError> '^commit|ci' (error at record.py:*)
  hgext/shelve.py: error importing: <SystemError> Parent module 'hgext' not loaded, cannot perform relative import (error at shelve.py:*)
  hgext/transplant.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  mercurial/encoding.py: error importing: <TypeError> bytes expected, not str (error at encoding.py:*)
  mercurial/fileset.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at registrar.py:*)
  mercurial/i18n.py: error importing: <TypeError> bytes expected, not str (error at i18n.py:*)
  mercurial/revset.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*)
  mercurial/scmwindows.py: error importing: <ImportError> No module named 'winreg' (error at scmwindows.py:*)
  mercurial/store.py: error importing: <TypeError> Can't convert 'bytes' object to str implicitly (error at store.py:*)
  mercurial/win32.py: error importing: <ImportError> No module named 'msvcrt' (error at win32.py:*)
  mercurial/windows.py: error importing: <ImportError> No module named 'msvcrt' (error at windows.py:*)

#endif

#if py3exe py3pygments
  $ hg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
