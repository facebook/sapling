#require test-repo

  $ cd "$TESTDIR"/..

  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs python contrib/check-py3-compat.py
  contrib/import-checker.py not using absolute_import
  contrib/import-checker.py requires print_function
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
  tests/filterpyflakes.py requires print_function
  tests/generate-working-copy-states.py requires print_function
  tests/get-with-headers.py requires print_function
  tests/heredoctest.py requires print_function
  tests/hypothesishelpers.py not using absolute_import
  tests/hypothesishelpers.py requires print_function
  tests/killdaemons.py not using absolute_import
  tests/md5sum.py not using absolute_import
  tests/mockblackbox.py not using absolute_import
  tests/printenv.py not using absolute_import
  tests/readlink.py not using absolute_import
  tests/readlink.py requires print_function
  tests/revlog-formatv0.py not using absolute_import
  tests/run-tests.py not using absolute_import
  tests/seq.py not using absolute_import
  tests/seq.py requires print_function
  tests/silenttestrunner.py not using absolute_import
  tests/silenttestrunner.py requires print_function
  tests/sitecustomize.py not using absolute_import
  tests/svn-safe-append.py not using absolute_import
  tests/svnxml.py not using absolute_import
  tests/test-ancestor.py requires print_function
  tests/test-atomictempfile.py not using absolute_import
  tests/test-batching.py not using absolute_import
  tests/test-batching.py requires print_function
  tests/test-bdiff.py not using absolute_import
  tests/test-bdiff.py requires print_function
  tests/test-context.py not using absolute_import
  tests/test-context.py requires print_function
  tests/test-demandimport.py not using absolute_import
  tests/test-demandimport.py requires print_function
  tests/test-doctest.py not using absolute_import
  tests/test-duplicateoptions.py not using absolute_import
  tests/test-duplicateoptions.py requires print_function
  tests/test-filecache.py not using absolute_import
  tests/test-filecache.py requires print_function
  tests/test-filelog.py not using absolute_import
  tests/test-filelog.py requires print_function
  tests/test-hg-parseurl.py not using absolute_import
  tests/test-hg-parseurl.py requires print_function
  tests/test-hgweb-auth.py not using absolute_import
  tests/test-hgweb-auth.py requires print_function
  tests/test-hgwebdir-paths.py not using absolute_import
  tests/test-hybridencode.py not using absolute_import
  tests/test-hybridencode.py requires print_function
  tests/test-lrucachedict.py not using absolute_import
  tests/test-lrucachedict.py requires print_function
  tests/test-manifest.py not using absolute_import
  tests/test-minirst.py not using absolute_import
  tests/test-minirst.py requires print_function
  tests/test-parseindex2.py not using absolute_import
  tests/test-parseindex2.py requires print_function
  tests/test-pathencode.py not using absolute_import
  tests/test-pathencode.py requires print_function
  tests/test-propertycache.py not using absolute_import
  tests/test-propertycache.py requires print_function
  tests/test-revlog-ancestry.py not using absolute_import
  tests/test-revlog-ancestry.py requires print_function
  tests/test-run-tests.py not using absolute_import
  tests/test-simplemerge.py not using absolute_import
  tests/test-status-inprocess.py not using absolute_import
  tests/test-status-inprocess.py requires print_function
  tests/test-symlink-os-yes-fs-no.py not using absolute_import
  tests/test-trusted.py not using absolute_import
  tests/test-trusted.py requires print_function
  tests/test-ui-color.py not using absolute_import
  tests/test-ui-color.py requires print_function
  tests/test-ui-config.py not using absolute_import
  tests/test-ui-config.py requires print_function
  tests/test-ui-verbosity.py not using absolute_import
  tests/test-ui-verbosity.py requires print_function
  tests/test-url.py not using absolute_import
  tests/test-url.py requires print_function
  tests/test-walkrepo.py requires print_function
  tests/test-wireproto.py requires print_function
  tests/tinyproxy.py requires print_function

#if py3exe
  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py
  contrib/check-code.py: invalid syntax: (unicode error) 'unicodeescape' codec can't decode bytes in position 18-19: malformed \N character escape (<unknown>, line 106)
  contrib/import-checker.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 569)
  contrib/revsetbenchmarks.py: invalid syntax: invalid syntax (<unknown>, line 186)
  doc/hgmanpage.py: invalid syntax: invalid syntax (<unknown>, line 286)
  hgext/acl.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/automv.py: error importing module: <SyntaxError> invalid syntax (commands.py, line 3324) (line 29)
  hgext/blackbox.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/bugzilla.py: error importing module: <ImportError> No module named 'urlparse' (line 284)
  hgext/censor.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/chgserver.py: error importing module: <ImportError> No module named 'SocketServer' (line 43)
  hgext/children.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/churn.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/clonebundles.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/color.py: invalid syntax: invalid syntax (<unknown>, line 551)
  hgext/convert/bzr.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line 18)
  hgext/convert/common.py: error importing module: <ImportError> No module named 'cPickle' (line 10)
  hgext/convert/convcmd.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/cvs.py: error importing module: <ImportError> No module named 'cStringIO' (line 9)
  hgext/convert/cvsps.py: error importing module: <ImportError> No module named 'cPickle' (line 9)
  hgext/convert/darcs.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/filemap.py: error importing module: <SystemError> Parent module 'hgext.convert' not loaded, cannot perform relative import (line 14)
  hgext/convert/git.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/gnuarch.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/hg.py: error importing module: <ImportError> No module named 'cStringIO' (line 21)
  hgext/convert/monotone.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/p4.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/convert/subversion.py: error importing module: <ImportError> No module named 'cPickle' (line 6)
  hgext/convert/transport.py: error importing module: <ImportError> No module named 'svn.client' (line 21)
  hgext/eol.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/extdiff.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/factotum.py: error importing: <ImportError> No module named 'cStringIO' (error at url.py:13)
  hgext/fetch.py: error importing module: <SyntaxError> invalid syntax (commands.py, line 3324) (line 12)
  hgext/fsmonitor/state.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/fsmonitor/watchmanclient.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/gpg.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/graphlog.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/hgcia.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/hgk.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/highlight/highlight.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/histedit.py: error importing module: <SyntaxError> invalid syntax (bundle2.py, line 977) (line 177)
  hgext/keyword.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/basestore.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/lfcommands.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/lfutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/localstore.py: error importing module: <ImportError> No module named 'lfutil' (line 13)
  hgext/largefiles/overrides.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/proto.py: error importing module: <ImportError> No module named 'urllib2' (line 7)
  hgext/largefiles/remotestore.py: error importing module: <ImportError> No module named 'urllib2' (line 9)
  hgext/largefiles/reposetup.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/largefiles/uisetup.py: error importing module: <SyntaxError> invalid syntax (archival.py, line 234) (line 11)
  hgext/largefiles/wirestore.py: error importing module: <ImportError> No module named 'lfutil' (line 8)
  hgext/mq.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/notify.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/pager.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/patchbomb.py: error importing module: <ImportError> No module named 'cStringIO' (line 68)
  hgext/purge.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/rebase.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/record.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/relink.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/schemes.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/share.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  hgext/shelve.py: error importing module: <SyntaxError> invalid syntax (bundle2.py, line 977) (line 28)
  hgext/strip.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  hgext/transplant.py: error importing: <SyntaxError> invalid syntax (bundle2.py, line 977) (error at bundlerepo.py:23)
  hgext/win32text.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/archival.py: invalid syntax: invalid syntax (<unknown>, line 234)
  mercurial/bookmarks.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/branchmap.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:10)
  mercurial/bundle2.py: invalid syntax: invalid syntax (<unknown>, line 977)
  mercurial/bundlerepo.py: error importing module: <SyntaxError> invalid syntax (bundle2.py, line 977) (line 23)
  mercurial/byterange.py: error importing module: <ImportError> No module named 'urllib2' (line 30)
  mercurial/changegroup.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:10)
  mercurial/changelog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/cmdutil.py: error importing module: <ImportError> No module named 'cStringIO' (line 10)
  mercurial/commands.py: invalid syntax: invalid syntax (<unknown>, line 3324)
  mercurial/commandserver.py: error importing module: <ImportError> No module named 'SocketServer' (line 10)
  mercurial/config.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/context.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/copies.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/crecord.py: error importing module: <ImportError> No module named 'cStringIO' (line 13)
  mercurial/destutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/dirstate.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/discovery.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/dispatch.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/exchange.py: error importing module: <ImportError> No module named 'urllib2' (line 12)
  mercurial/extensions.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/filelog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/filemerge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/fileset.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/formatter.py: error importing module: <ImportError> No module named 'cPickle' (line 10)
  mercurial/graphmod.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/help.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/hg.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/hgweb/common.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line 11)
  mercurial/hgweb/hgweb_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 14)
  mercurial/hgweb/hgwebdir_mod.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 15)
  mercurial/hgweb/protocol.py: error importing module: <ImportError> No module named 'cStringIO' (line 10)
  mercurial/hgweb/request.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 15)
  mercurial/hgweb/server.py: error importing module: <ImportError> No module named 'BaseHTTPServer' (line 11)
  mercurial/hgweb/webcommands.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 16)
  mercurial/hgweb/webutil.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 16)
  mercurial/hgweb/wsgicgi.py: error importing module: <SystemError> Parent module 'mercurial.hgweb' not loaded, cannot perform relative import (line 16)
  mercurial/hook.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/httpclient/_readers.py: error importing module: <ImportError> No module named 'httplib' (line 36)
  mercurial/httpconnection.py: error importing module: <ImportError> No module named 'urllib2' (line 17)
  mercurial/httppeer.py: error importing module: <ImportError> No module named 'httplib' (line 12)
  mercurial/keepalive.py: error importing module: <ImportError> No module named 'httplib' (line 113)
  mercurial/localrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/lock.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/mail.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/manifest.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/match.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/mdiff.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/merge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/minirst.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/namespaces.py: error importing: <ImportError> No module named 'cStringIO' (error at patch.py:11)
  mercurial/obsolete.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/patch.py: error importing module: <ImportError> No module named 'cStringIO' (line 11)
  mercurial/pathutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/peer.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/pure/mpatch.py: error importing module: <ImportError> No module named 'cStringIO' (line 10)
  mercurial/pure/parsers.py: error importing module: <ImportError> No module named 'cStringIO' (line 10)
  mercurial/pushkey.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/pvec.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/registrar.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/repair.py: error importing module: <SyntaxError> invalid syntax (bundle2.py, line 977) (line 15)
  mercurial/repoview.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/revlog.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/revset.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/scmutil.py: error importing module: <ImportError> No module named 'Queue' (line 10)
  mercurial/scmwindows.py: error importing module: <ImportError> No module named '_winreg' (line 3)
  mercurial/similar.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/simplemerge.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/sshpeer.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/sshserver.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/sslutil.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/statichttprepo.py: error importing module: <ImportError> No module named 'urllib2' (line 15)
  mercurial/store.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/streamclone.py: error importing: <ImportError> No module named 'Queue' (error at scmutil.py:10)
  mercurial/subrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at cmdutil.py:10)
  mercurial/tagmerge.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/tags.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/templatefilters.py: error importing: <ImportError> No module named 'cStringIO' (error at patch.py:11)
  mercurial/templatekw.py: error importing: <ImportError> No module named 'cStringIO' (error at patch.py:11)
  mercurial/templater.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/transaction.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/ui.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/unionrepo.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/url.py: error importing module: <ImportError> No module named 'cStringIO' (line 13)
  mercurial/util.py: error importing: <ImportError> No module named 'cStringIO' (error at parsers.py:10)
  mercurial/verify.py: error importing: <ImportError> No module named 'cStringIO' (error at mpatch.py:10)
  mercurial/win32.py: error importing module: <ImportError> No module named 'msvcrt' (line 12)
  mercurial/windows.py: error importing module: <ImportError> No module named '_winreg' (line 10)
  mercurial/wireproto.py: error importing module: <SyntaxError> invalid syntax (bundle2.py, line 977) (line 22)
  tests/filterpyflakes.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 61)
  tests/generate-working-copy-states.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 69)
  tests/get-with-headers.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 44)
  tests/readlink.py: invalid syntax: invalid syntax (<unknown>, line 7)
  tests/seq.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 23)
  tests/silenttestrunner.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 11)
  tests/test-ancestor.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 187)
  tests/test-batching.py: invalid syntax: invalid syntax (<unknown>, line 34)
  tests/test-bdiff.py: invalid syntax: invalid syntax (<unknown>, line 10)
  tests/test-context.py: invalid syntax: invalid syntax (<unknown>, line 21)
  tests/test-demandimport.py: invalid syntax: invalid syntax (<unknown>, line 26)
  tests/test-duplicateoptions.py: invalid syntax: invalid syntax (<unknown>, line 34)
  tests/test-filecache.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 23)
  tests/test-filelog.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 33)
  tests/test-hg-parseurl.py: invalid syntax: invalid syntax (<unknown>, line 4)
  tests/test-hgweb-auth.py: invalid syntax: invalid syntax (<unknown>, line 24)
  tests/test-hybridencode.py: invalid syntax: invalid syntax (<unknown>, line 5)
  tests/test-lrucachedict.py: invalid syntax: invalid syntax (<unknown>, line 6)
  tests/test-minirst.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 6)
  tests/test-parseindex2.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 173)
  tests/test-propertycache.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 50)
  tests/test-revlog-ancestry.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 49)
  tests/test-status-inprocess.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 8)
  tests/test-trusted.py: invalid syntax: invalid syntax (<unknown>, line 60)
  tests/test-ui-color.py: invalid syntax: invalid syntax (<unknown>, line 11)
  tests/test-ui-config.py: invalid syntax: invalid syntax (<unknown>, line 32)
  tests/test-ui-verbosity.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 9)
  tests/test-walkrepo.py: invalid syntax: invalid syntax (<unknown>, line 37)
  tests/test-wireproto.py: invalid syntax: invalid syntax (<unknown>, line 55)
  tests/tinyproxy.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line 53)

#endif
