#require test-repo

  $ cd "$TESTDIR"/..

  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs python contrib/check-py3-compat.py
  hgext/fetch.py not using absolute_import
  hgext/fsmonitor/pywatchman/__init__.py not using absolute_import
  hgext/fsmonitor/pywatchman/__init__.py requires print_function
  hgext/fsmonitor/pywatchman/capabilities.py not using absolute_import
  hgext/fsmonitor/pywatchman/pybser.py not using absolute_import
  hgext/gpg.py not using absolute_import
  hgext/graphlog.py not using absolute_import
  hgext/hgcia.py not using absolute_import
  hgext/hgk.py not using absolute_import
  hgext/highlight/__init__.py not using absolute_import
  hgext/highlight/highlight.py not using absolute_import
  hgext/histedit.py not using absolute_import
  hgext/largefiles/__init__.py not using absolute_import
  hgext/largefiles/basestore.py not using absolute_import
  hgext/largefiles/lfcommands.py not using absolute_import
  hgext/largefiles/lfutil.py not using absolute_import
  hgext/largefiles/localstore.py not using absolute_import
  hgext/largefiles/overrides.py not using absolute_import
  hgext/largefiles/proto.py not using absolute_import
  hgext/largefiles/remotestore.py not using absolute_import
  hgext/largefiles/reposetup.py not using absolute_import
  hgext/largefiles/uisetup.py not using absolute_import
  hgext/largefiles/wirestore.py not using absolute_import
  hgext/mq.py not using absolute_import
  hgext/rebase.py not using absolute_import
  hgext/share.py not using absolute_import
  hgext/win32text.py not using absolute_import
  i18n/check-translation.py not using absolute_import
  i18n/polib.py not using absolute_import
  setup.py not using absolute_import
  tests/heredoctest.py requires print_function
  tests/md5sum.py not using absolute_import
  tests/readlink.py not using absolute_import
  tests/readlink.py requires print_function
  tests/run-tests.py not using absolute_import
  tests/svn-safe-append.py not using absolute_import
  tests/test-atomictempfile.py not using absolute_import
  tests/test-demandimport.py not using absolute_import

#if py3exe
  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py
  contrib/check-code.py: invalid syntax: (unicode error) 'unicodeescape' codec can't decode bytes in position *-*: malformed \N character escape (<unknown>, line *) (glob)
  doc/hgmanpage.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  hgext/automv.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/blackbox.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/bugzilla.py: error importing module: <ImportError> No module named 'urlparse' (line *) (glob)
  hgext/censor.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/chgserver.py: error importing module: <ImportError> No module named 'SocketServer' (line *) (glob)
  hgext/children.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/churn.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/clonebundles.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/color.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  hgext/convert/bzr.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/common.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/convcmd.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/convert/cvs.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/cvsps.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/darcs.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/filemap.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/git.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/gnuarch.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/hg.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/convert/monotone.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/p*.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/subversion.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/transport.py: error importing module: <ImportError> No module named 'svn.client' (line *) (glob)
  hgext/eol.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/extdiff.py: error importing module: <SyntaxError> invalid syntax (archival.py, line *) (line *) (glob)
  hgext/factotum.py: error importing: <ImportError> No module named 'httplib' (error at __init__.py:*) (glob)
  hgext/fetch.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/fsmonitor/watchmanclient.py: error importing module: <SystemError> Parent module 'hgext.fsmonitor' not loaded, cannot perform relative import (line *) (glob)
  hgext/gpg.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/graphlog.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/hgcia.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/hgk.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/histedit.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  hgext/keyword.py: error importing: <ImportError> No module named 'BaseHTTPServer' (error at common.py:*) (glob)
  hgext/largefiles/basestore.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/largefiles/lfcommands.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/largefiles/lfutil.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/largefiles/localstore.py: error importing module: <ImportError> No module named 'lfutil' (line *) (glob)
  hgext/largefiles/overrides.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/largefiles/proto.py: error importing: <ImportError> No module named 'httplib' (error at httppeer.py:*) (glob)
  hgext/largefiles/remotestore.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at wireproto.py:*) (glob)
  hgext/largefiles/reposetup.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/largefiles/uisetup.py: error importing module: <SyntaxError> invalid syntax (archival.py, line *) (line *) (glob)
  hgext/largefiles/wirestore.py: error importing module: <ImportError> No module named 'lfutil' (line *) (glob)
  hgext/mq.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/notify.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/pager.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/patchbomb.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/purge.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/rebase.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/record.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/relink.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/schemes.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/share.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/shelve.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  hgext/strip.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  hgext/transplant.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  mercurial/archival.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/branchmap.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/bundle*.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/bundlerepo.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  mercurial/changegroup.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/changelog.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/cmdutil.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/commands.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/commandserver.py: error importing module: <ImportError> No module named 'SocketServer' (line *) (glob)
  mercurial/context.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/copies.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/crecord.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/dirstate.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/discovery.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/dispatch.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/exchange.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  mercurial/extensions.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/filelog.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/filemerge.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/fileset.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/formatter.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  mercurial/graphmod.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/help.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/hg.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  mercurial/hgweb/common.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line *) (glob)
  mercurial/hgweb/hgweb_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/hgwebdir_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/protocol.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/request.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/server.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line *) (glob)
  mercurial/hgweb/webcommands.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/webutil.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/wsgicgi.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hook.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/httpclient/_readers.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/httpconnection.py: error importing: <ImportError> No module named 'httplib' (error at __init__.py:*) (glob)
  mercurial/httppeer.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/keepalive.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/localrepo.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/mail.py: error importing module: <AttributeError> module 'email' has no attribute 'Header' (line *) (glob)
  mercurial/manifest.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/merge.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/namespaces.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/patch.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/pure/mpatch.py: error importing module: <ImportError> cannot import name 'pycompat' (line *) (glob)
  mercurial/pure/parsers.py: error importing module: <ImportError> No module named 'mercurial.pure.node' (line *) (glob)
  mercurial/repair.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  mercurial/revlog.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/revset.py: error importing module: <AttributeError> 'dict' object has no attribute 'iteritems' (line *) (glob)
  mercurial/scmutil.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/scmwindows.py: error importing module: <ImportError> No module named '_winreg' (line *) (glob)
  mercurial/simplemerge.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/sshpeer.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at wireproto.py:*) (glob)
  mercurial/sshserver.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/statichttprepo.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/store.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/streamclone.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/subrepo.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/templatefilters.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/templatekw.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/templater.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/ui.py: error importing: <ImportError> No module named 'cPickle' (error at formatter.py:*) (glob)
  mercurial/unionrepo.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/url.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/verify.py: error importing: <AttributeError> 'dict' object has no attribute 'iteritems' (error at revset.py:*) (glob)
  mercurial/win*.py: error importing module: <ImportError> No module named 'msvcrt' (line *) (glob)
  mercurial/windows.py: error importing module: <ImportError> No module named '_winreg' (line *) (glob)
  mercurial/wireproto.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  tests/readlink.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)

#endif
