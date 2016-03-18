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
  hgext/color.py: invalid syntax: invalid syntax (<unknown>, line 551)
  mercurial/archival.py: invalid syntax: invalid syntax (<unknown>, line 234)
  mercurial/bundle2.py: invalid syntax: invalid syntax (<unknown>, line 977)
  mercurial/commands.py: invalid syntax: invalid syntax (<unknown>, line 3324)
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
