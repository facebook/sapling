#require test-repo

  $ cd "$TESTDIR"/..

  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs python contrib/check-py3-compat.py
  doc/check-seclevel.py not using absolute_import
  doc/gendoc.py not using absolute_import
  doc/hgmanpage.py not using absolute_import
  hgext/color.py not using absolute_import
  hgext/eol.py not using absolute_import
  hgext/extdiff.py not using absolute_import
  hgext/factotum.py not using absolute_import
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
  tests/killdaemons.py not using absolute_import
  tests/md5sum.py not using absolute_import
  tests/mockblackbox.py not using absolute_import
  tests/printenv.py not using absolute_import
  tests/readlink.py not using absolute_import
  tests/readlink.py requires print_function
  tests/revlog-formatv0.py not using absolute_import
  tests/run-tests.py not using absolute_import
  tests/sitecustomize.py not using absolute_import
  tests/svn-safe-append.py not using absolute_import
  tests/svnxml.py not using absolute_import
  tests/test-atomictempfile.py not using absolute_import
  tests/test-demandimport.py not using absolute_import
  tests/test-demandimport.py requires print_function
  tests/test-doctest.py not using absolute_import
  tests/test-hgwebdir-paths.py not using absolute_import
  tests/test-lrucachedict.py not using absolute_import
  tests/test-lrucachedict.py requires print_function
  tests/test-manifest.py not using absolute_import
  tests/test-pathencode.py not using absolute_import
  tests/test-pathencode.py requires print_function
  tests/test-revlog-ancestry.py requires print_function
  tests/test-run-tests.py not using absolute_import
  tests/test-simplemerge.py not using absolute_import
  tests/test-status-inprocess.py not using absolute_import
  tests/test-status-inprocess.py requires print_function
  tests/test-symlink-os-yes-fs-no.py not using absolute_import
  tests/test-trusted.py not using absolute_import
  tests/test-trusted.py requires print_function
  tests/test-ui-color.py not using absolute_import
  tests/test-url.py not using absolute_import

#if py3exe
  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py
  contrib/check-code.py: invalid syntax: (unicode error) 'unicodeescape' codec can't decode bytes in position *-*: malformed \N character escape (<unknown>, line *) (glob)
  doc/hgmanpage.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  hgext/acl.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/automv.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/blackbox.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/bugzilla.py: error importing module: <ImportError> No module named 'urlparse' (line *) (glob)
  hgext/censor.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/chgserver.py: error importing module: <ImportError> No module named 'SocketServer' (line *) (glob)
  hgext/children.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/churn.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/clonebundles.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/color.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  hgext/convert/bzr.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/common.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/convcmd.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/cvs.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  hgext/convert/cvsps.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/darcs.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/filemap.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line *) (glob)
  hgext/convert/git.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/gnuarch.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/hg.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  hgext/convert/monotone.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/p*.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/convert/subversion.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  hgext/convert/transport.py: error importing module: <ImportError> No module named 'svn.client' (line *) (glob)
  hgext/eol.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/extdiff.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/factotum.py: error importing: <ImportError> No module named 'cStringIO' (error at url.py:*) (glob)
  hgext/fetch.py: error importing module: <SyntaxError> invalid syntax (commands.py, line *) (line *) (glob)
  hgext/fsmonitor/state.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/fsmonitor/watchmanclient.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/gpg.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/graphlog.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/hgcia.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/hgk.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/highlight/highlight.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/histedit.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  hgext/keyword.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/basestore.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/lfcommands.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/lfutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/localstore.py: error importing module: <ImportError> No module named 'lfutil' (line *) (glob)
  hgext/largefiles/overrides.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/proto.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  hgext/largefiles/remotestore.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  hgext/largefiles/reposetup.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/largefiles/uisetup.py: error importing module: <SyntaxError> invalid syntax (archival.py, line *) (line *) (glob)
  hgext/largefiles/wirestore.py: error importing module: <ImportError> No module named 'lfutil' (line *) (glob)
  hgext/mq.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/notify.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/pager.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/patchbomb.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  hgext/purge.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/rebase.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/record.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/relink.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/schemes.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/share.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  hgext/shelve.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  hgext/strip.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  hgext/transplant.py: error importing: <SyntaxError> invalid syntax (bundle*.py, line *) (error at bundlerepo.py:*) (glob)
  hgext/win*text.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/archival.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/bookmarks.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/branchmap.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:*) (glob)
  mercurial/bundle*.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/bundlerepo.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  mercurial/byterange.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  mercurial/changegroup.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:*) (glob)
  mercurial/changelog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/cmdutil.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/commands.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  mercurial/commandserver.py: error importing module: <ImportError> No module named 'SocketServer' (line *) (glob)
  mercurial/config.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/context.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/copies.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/crecord.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/destutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/dirstate.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/discovery.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/dispatch.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/exchange.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  mercurial/extensions.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/filelog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/filemerge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/fileset.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/formatter.py: error importing module: <ImportError> No module named 'cPickle' (line *) (glob)
  mercurial/graphmod.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/help.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/hg.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/hgweb/common.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line *) (glob)
  mercurial/hgweb/hgweb_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/hgwebdir_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/protocol.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/hgweb/request.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/server.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line *) (glob)
  mercurial/hgweb/webcommands.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/webutil.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hgweb/wsgicgi.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line *) (glob)
  mercurial/hook.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/httpclient/_readers.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/httpconnection.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  mercurial/httppeer.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/keepalive.py: error importing module: <ImportError> No module named 'httplib' (line *) (glob)
  mercurial/localrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/lock.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/mail.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/manifest.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/match.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/mdiff.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/merge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/minirst.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/namespaces.py: error importing: <ImportError> No module named 'cStringIO' (error at patch.py:*) (glob)
  mercurial/obsolete.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/patch.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/pathutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/peer.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/pure/mpatch.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/pure/parsers.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/pushkey.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/pvec.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/registrar.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/repair.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  mercurial/repoview.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/revlog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/revset.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/scmutil.py: error importing module: <ImportError> No module named 'Queue' (line *) (glob)
  mercurial/scmwindows.py: error importing module: <ImportError> No module named '_winreg' (line *) (glob)
  mercurial/similar.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/simplemerge.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/sshpeer.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/sshserver.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/sslutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/statichttprepo.py: error importing module: <ImportError> No module named 'urllib2' (line *) (glob)
  mercurial/store.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/streamclone.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:*) (glob)
  mercurial/subrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:*) (glob)
  mercurial/tagmerge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/tags.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/templatefilters.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/templatekw.py: error importing: <ImportError> No module named 'cStringIO' (error at patch.py:*) (glob)
  mercurial/templater.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/transaction.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/ui.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/unionrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/url.py: error importing module: <ImportError> No module named 'cStringIO' (line *) (glob)
  mercurial/util.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:*) (glob)
  mercurial/verify.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:*) (glob)
  mercurial/win*.py: error importing module: <ImportError> No module named 'msvcrt' (line *) (glob)
  mercurial/windows.py: error importing module: <ImportError> No module named '_winreg' (line *) (glob)
  mercurial/wireproto.py: error importing module: <SyntaxError> invalid syntax (bundle*.py, line *) (line *) (glob)
  tests/readlink.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  tests/test-demandimport.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  tests/test-lrucachedict.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)
  tests/test-revlog-ancestry.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *) (glob)
  tests/test-status-inprocess.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *) (glob)
  tests/test-trusted.py: invalid syntax: invalid syntax (<unknown>, line *) (glob)

#endif
