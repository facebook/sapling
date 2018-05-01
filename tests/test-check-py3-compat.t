#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

  $ testrepohg files 'set:(**.py)' \
  > -X hgdemandimport/demandimportpy2.py \
  > -X hg-git \
  > | sed 's|\\|/|g' | xargs $PYTHON contrib/check-py3-compat.py
  contrib/hggitperf.py not using absolute_import
  contrib/python-zstandard/setup.py not using absolute_import
  contrib/python-zstandard/setup_zstd.py not using absolute_import
  contrib/python-zstandard/tests/common.py not using absolute_import
  contrib/python-zstandard/tests/test_buffer_util.py not using absolute_import
  contrib/python-zstandard/tests/test_compressor.py not using absolute_import
  contrib/python-zstandard/tests/test_compressor_fuzzing.py not using absolute_import
  contrib/python-zstandard/tests/test_data_structures.py not using absolute_import
  contrib/python-zstandard/tests/test_data_structures_fuzzing.py not using absolute_import
  contrib/python-zstandard/tests/test_decompressor.py not using absolute_import
  contrib/python-zstandard/tests/test_decompressor_fuzzing.py not using absolute_import
  contrib/python-zstandard/tests/test_estimate_sizes.py not using absolute_import
  contrib/python-zstandard/tests/test_module_attributes.py not using absolute_import
  contrib/python-zstandard/tests/test_train_dictionary.py not using absolute_import
  fb/packaging/build_deb.py not using absolute_import
  fb/tests/sqldirstate_benchmark.py not using absolute_import
  fb/tests/sqldirstate_benchmark.py requires print_function
  hgext/arcdiff.py not using absolute_import
  hgext/backups.py not using absolute_import
  hgext/catnotate.py not using absolute_import
  hgext/checkmessagehook.py not using absolute_import
  hgext/chistedit.py not using absolute_import
  hgext/copytrace.py not using absolute_import
  hgext/debugcommitmessage.py not using absolute_import
  hgext/dialect.py not using absolute_import
  hgext/directaccess.py not using absolute_import
  hgext/drop.py not using absolute_import
  hgext/edrecord.py not using absolute_import
  hgext/extorder.py not using absolute_import
  hgext/fastannotate/error.py not using absolute_import
  hgext/fastannotate/formatter.py not using absolute_import
  hgext/fastannotate/protocol.py not using absolute_import
  hgext/fastlog.py not using absolute_import
  hgext/fastpartialmatch.py not using absolute_import
  hgext/fbconduit.py not using absolute_import
  hgext/fbhistedit.py not using absolute_import
  hgext/fbshow.py not using absolute_import
  hgext/fbsparse.py not using absolute_import
  hgext/generic_bisect.py not using absolute_import
  hgext/githelp.py not using absolute_import
  hgext/gitlookup.py not using absolute_import
  hgext/grepdiff.py not using absolute_import
  hgext/grpcheck.py not using absolute_import
  hgext/hggit/__init__.py not using absolute_import
  hgext/hggit/_ssh.py not using absolute_import
  hgext/hggit/compat.py not using absolute_import
  hgext/hggit/git2hg.py not using absolute_import
  hgext/hggit/git_handler.py not using absolute_import
  hgext/hggit/gitdirstate.py not using absolute_import
  hgext/hggit/gitrepo.py not using absolute_import
  hgext/hggit/hg2git.py not using absolute_import
  hgext/hggit/hgrepo.py not using absolute_import
  hgext/hggit/overlay.py not using absolute_import
  hgext/hggit/util.py not using absolute_import
  hgext/hggit/verify.py not using absolute_import
  hgext/hgsubversion/__init__.py not using absolute_import
  hgext/hgsubversion/compathacks.py not using absolute_import
  hgext/hgsubversion/editor.py not using absolute_import
  hgext/hgsubversion/hooks/updatemeta.py not using absolute_import
  hgext/hgsubversion/layouts/__init__.py not using absolute_import
  hgext/hgsubversion/layouts/base.py not using absolute_import
  hgext/hgsubversion/layouts/custom.py not using absolute_import
  hgext/hgsubversion/layouts/single.py not using absolute_import
  hgext/hgsubversion/layouts/standard.py not using absolute_import
  hgext/hgsubversion/maps.py not using absolute_import
  hgext/hgsubversion/pushmod.py not using absolute_import
  hgext/hgsubversion/replay.py not using absolute_import
  hgext/hgsubversion/stupid.py not using absolute_import
  hgext/hgsubversion/svncommands.py not using absolute_import
  hgext/hgsubversion/svnexternals.py not using absolute_import
  hgext/hgsubversion/svnmeta.py not using absolute_import
  hgext/hgsubversion/svnrepo.py not using absolute_import
  hgext/hgsubversion/svnwrap/__init__.py not using absolute_import
  hgext/hgsubversion/svnwrap/common.py not using absolute_import
  hgext/hgsubversion/svnwrap/subvertpy_wrapper.py not using absolute_import
  hgext/hgsubversion/svnwrap/svn_swig_wrapper.py not using absolute_import
  hgext/hgsubversion/util.py not using absolute_import
  hgext/hgsubversion/verify.py not using absolute_import
  hgext/hgsubversion/wrappers.py not using absolute_import
  hgext/infinitepush/bundleparts.py not using absolute_import
  hgext/infinitepush/common.py not using absolute_import
  hgext/infinitepush/fileindexapi.py not using absolute_import
  hgext/infinitepush/indexapi.py not using absolute_import
  hgext/infinitepush/sqlindexapi.py not using absolute_import
  hgext/infinitepush/store.py not using absolute_import
  hgext/linkrevcache.py not using absolute_import
  hgext/logginghelper.py not using absolute_import
  hgext/morestatus.py not using absolute_import
  hgext/myparent.py not using absolute_import
  hgext/nointerrupt.py not using absolute_import
  hgext/ownercheck.py not using absolute_import
  hgext/perftweaks.py not using absolute_import
  hgext/phabdiff.py not using absolute_import
  hgext/phabstatus.py not using absolute_import
  hgext/phrevset.py not using absolute_import
  hgext/pullcreatemarkers.py not using absolute_import
  hgext/rage.py not using absolute_import
  hgext/remoteid.py not using absolute_import
  hgext/remotenames.py not using absolute_import
  hgext/reset.py not using absolute_import
  hgext/sampling.py not using absolute_import
  hgext/sigtrace.py not using absolute_import
  hgext/simplecache.py not using absolute_import
  hgext/sshaskpass.py not using absolute_import
  hgext/stat.py not using absolute_import
  hgext/upgradegeneraldelta.py not using absolute_import
  hgext/whereami.py not using absolute_import
  lib/argparse/src/hg_dump_commands_ext.py not using absolute_import
  tests/bundlerepologger.py not using absolute_import
  tests/comprehensive/test-hgsubversion-custom-layout.py not using absolute_import
  tests/comprehensive/test-hgsubversion-obsstore-on.py not using absolute_import
  tests/comprehensive/test-hgsubversion-rebuildmeta.py not using absolute_import
  tests/comprehensive/test-hgsubversion-sqlite-revmap.py not using absolute_import
  tests/comprehensive/test-hgsubversion-stupid-pull.py not using absolute_import
  tests/comprehensive/test-hgsubversion-updatemeta.py not using absolute_import
  tests/comprehensive/test-hgsubversion-verify-and-startrev.py not using absolute_import
  tests/conduithttp.py not using absolute_import
  tests/dummyext1.py not using absolute_import
  tests/dummyext2.py not using absolute_import
  tests/fixtures/rsvn.py not using absolute_import
  tests/fixtures/rsvn.py requires print_function
  tests/hggit/commitextra.py not using absolute_import
  tests/test-fb-hgext-extutil.py not using absolute_import
  tests/test-fb-hgext-fastmanifest.py not using absolute_import
  tests/test-fb-hgext-generic-bisect.py not using absolute_import
  tests/test-fb-hgext-sshaskpass.py not using absolute_import
  tests/test-hggit-url-parsing.py not using absolute_import
  tests/test-hggit-url-parsing.py requires print_function
  tests/test-hgsubversion-binaryfiles.py not using absolute_import
  tests/test-hgsubversion-diff.py not using absolute_import
  tests/test-hgsubversion-externals.py not using absolute_import
  tests/test-hgsubversion-externals.py requires print_function
  tests/test-hgsubversion-fetch-branches.py not using absolute_import
  tests/test-hgsubversion-fetch-command-regexes.py not using absolute_import
  tests/test-hgsubversion-fetch-command.py not using absolute_import
  tests/test-hgsubversion-fetch-dir-removal.py not using absolute_import
  tests/test-hgsubversion-fetch-exec.py not using absolute_import
  tests/test-hgsubversion-fetch-mappings.py not using absolute_import
  tests/test-hgsubversion-fetch-renames.py not using absolute_import
  tests/test-hgsubversion-fetch-symlinks.py not using absolute_import
  tests/test-hgsubversion-fetch-truncated.py not using absolute_import
  tests/test-hgsubversion-helpers.py not using absolute_import
  tests/test-hgsubversion-hooks.py not using absolute_import
  tests/test-hgsubversion-pull-fallback.py not using absolute_import
  tests/test-hgsubversion-pull.py not using absolute_import
  tests/test-hgsubversion-push-autoprops.py not using absolute_import
  tests/test-hgsubversion-push-command.py not using absolute_import
  tests/test-hgsubversion-push-dirs.py not using absolute_import
  tests/test-hgsubversion-push-eol.py not using absolute_import
  tests/test-hgsubversion-push-renames.py not using absolute_import
  tests/test-hgsubversion-revmap-migrate.py not using absolute_import
  tests/test-hgsubversion-single-dir-clone.py not using absolute_import
  tests/test-hgsubversion-single-dir-clone.py requires print_function
  tests/test-hgsubversion-single-dir-push.py not using absolute_import
  tests/test-hgsubversion-svn-pre-commit-hooks.py not using absolute_import
  tests/test-hgsubversion-svnwrap.py not using absolute_import
  tests/test-hgsubversion-tags.py not using absolute_import
  tests/test-hgsubversion-template-keywords.py not using absolute_import
  tests/test-hgsubversion-template-keywords.py requires print_function
  tests/test-hgsubversion-unaffected-core.py not using absolute_import
  tests/test-hgsubversion-urls.py not using absolute_import
  tests/test-hgsubversion-utility-commands.py not using absolute_import
  tests/test_hgsubversion_util.py not using absolute_import
  tests/test_hgsubversion_util.py requires print_function
  tests/waitforfile.py not using absolute_import

#if py3exe
  $ testrepohg files 'set:(**.py) - grep(pygments)' \
  > -X hgdemandimport/demandimportpy2.py \
  > -X hgext/fsmonitor/pywatchman \
  > -X hg-git \
  > | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
  fb-hgext/scripts/lint.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  fb-hgext/tests/get-with-headers.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  fb-hgext/tests/heredoctest.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  hgext/convert/transport.py: error importing: <*Error> No module named 'svn.client' (error at transport.py:*) (glob)
  hgext/hgsql.py: error importing: <ModuleNotFoundError> No module named 'Queue' (error at hgsql.py:*)
  hgext/lz4revlog.py: error importing: <ModuleNotFoundError> No module named 'lz4' (error at lz4revlog.py:*)
  hgext/remotenames.py: error importing: <ModuleNotFoundError> No module named 'UserDict' (error at remotenames.py:*)
  hgsubversion/hgsubversion/compathacks.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/editor.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/hooks/updatemeta.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/maps.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/pushmod.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/stupid.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svncommands.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svnmeta.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svnrepo.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svnwrap/__init__.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svnwrap/subvertpy_wrapper.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/svnwrap/svn_swig_wrapper.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/hgsubversion/util.py: invalid syntax: invalid token (<unknown>, line *)
  hgsubversion/hgsubversion/wrappers.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/tests/fixtures/rsvn.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  hgsubversion/tests/test_externals.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/tests/test_push_command.py: invalid syntax: invalid syntax (<unknown>, line *)
  hgsubversion/tests/test_single_dir_clone.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  hgsubversion/tests/test_svn_pre_commit_hooks.py: invalid syntax: invalid token (<unknown>, line *)
  hgsubversion/tests/test_template_keywords.py: invalid syntax: Missing parentheses in call to 'print' (<unknown>, line *)
  hgsubversion/tests/test_util.py: invalid syntax: invalid syntax (<unknown>, line *)
  mercurial/cffi/bdiff.py: error importing: <ImportError> cannot import name '_bdiff' (error at bdiff.py:*)
  mercurial/cffi/bdiffbuild.py: error importing: <ModuleNotFoundError> No module named 'cffi' (error at bdiffbuild.py:*)
  mercurial/cffi/mpatch.py: error importing: <ImportError> cannot import name '_mpatch' (error at mpatch.py:*)
  mercurial/cffi/mpatchbuild.py: error importing: <ModuleNotFoundError> No module named 'cffi' (error at mpatchbuild.py:*)
  mercurial/cffi/osutilbuild.py: error importing: <ModuleNotFoundError> No module named 'cffi' (error at osutilbuild.py:*)
  mercurial/scmwindows.py: error importing: <*Error> No module named 'msvcrt' (error at win32.py:*) (glob)
  mercurial/win32.py: error importing: <*Error> No module named 'msvcrt' (error at win32.py:*) (glob)
  mercurial/windows.py: error importing: <*Error> No module named 'msvcrt' (error at windows.py:*) (glob)

#endif

#if py3exe py3pygments
  $ testrepohg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
  hg-git/tests/hghave.py: invalid syntax: invalid token (<unknown>, line *)
#endif
