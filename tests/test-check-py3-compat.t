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
  fb-hgext/infinitepush/bundleparts.py not using absolute_import
  fb-hgext/infinitepush/common.py not using absolute_import
  fb-hgext/infinitepush/fileindexapi.py not using absolute_import
  fb-hgext/infinitepush/indexapi.py not using absolute_import
  fb-hgext/infinitepush/sqlindexapi.py not using absolute_import
  fb-hgext/infinitepush/store.py not using absolute_import
  fb-hgext/phabricator/arcconfig.py not using absolute_import
  fb-hgext/phabricator/diffprops.py not using absolute_import
  fb-hgext/phabricator/graphql.py not using absolute_import
  fb-hgext/phabricator/phabricator_graphql_client_requests.py not using absolute_import
  fb-hgext/phabricator/phabricator_graphql_client_urllib.py not using absolute_import
  fb-hgext/scripts/lint.py not using absolute_import
  fb-hgext/scripts/lint.py requires print_function
  fb-hgext/scripts/unit.py not using absolute_import
  fb-hgext/scripts/utils.py not using absolute_import
  fb-hgext/setup.py not using absolute_import
  fb-hgext/tests/bundlerepologger.py not using absolute_import
  fb-hgext/tests/get-with-headers.py not using absolute_import
  fb-hgext/tests/get-with-headers.py requires print_function
  fb-hgext/tests/getflogheads.py not using absolute_import
  fb-hgext/tests/heredoctest.py not using absolute_import
  fb-hgext/tests/heredoctest.py requires print_function
  fb-hgext/tests/killdaemons.py not using absolute_import
  fb-hgext/tests/ls-l.py not using absolute_import
  fb-hgext/tests/ls-l.py requires print_function
  fb-hgext/tests/perftest.py not using absolute_import
  fb-hgext/tests/perftest.py requires print_function
  fb-hgext/tests/treemanifest_correctness.py not using absolute_import
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
  hgext/errorredirect.py not using absolute_import
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
  hgext/linkrevcache.py not using absolute_import
  hgext/logginghelper.py not using absolute_import
  hgext/morestatus.py not using absolute_import
  hgext/myparent.py not using absolute_import
  hgext/nointerrupt.py not using absolute_import
  hgext/ownercheck.py not using absolute_import
  hgext/p4fastimport/filetransaction.py not using absolute_import
  hgext/patchpython.py not using absolute_import
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
  hgsubversion/hgsubversion/__init__.py not using absolute_import
  hgsubversion/hgsubversion/compathacks.py not using absolute_import
  hgsubversion/hgsubversion/editor.py not using absolute_import
  hgsubversion/hgsubversion/hooks/updatemeta.py not using absolute_import
  hgsubversion/hgsubversion/layouts/__init__.py not using absolute_import
  hgsubversion/hgsubversion/layouts/base.py not using absolute_import
  hgsubversion/hgsubversion/layouts/custom.py not using absolute_import
  hgsubversion/hgsubversion/layouts/single.py not using absolute_import
  hgsubversion/hgsubversion/layouts/standard.py not using absolute_import
  hgsubversion/hgsubversion/maps.py not using absolute_import
  hgsubversion/hgsubversion/pushmod.py not using absolute_import
  hgsubversion/hgsubversion/replay.py not using absolute_import
  hgsubversion/hgsubversion/stupid.py not using absolute_import
  hgsubversion/hgsubversion/svncommands.py not using absolute_import
  hgsubversion/hgsubversion/svnexternals.py not using absolute_import
  hgsubversion/hgsubversion/svnmeta.py not using absolute_import
  hgsubversion/hgsubversion/svnrepo.py not using absolute_import
  hgsubversion/hgsubversion/svnwrap/__init__.py not using absolute_import
  hgsubversion/hgsubversion/svnwrap/common.py not using absolute_import
  hgsubversion/hgsubversion/svnwrap/subvertpy_wrapper.py not using absolute_import
  hgsubversion/hgsubversion/svnwrap/svn_swig_wrapper.py not using absolute_import
  hgsubversion/hgsubversion/util.py not using absolute_import
  hgsubversion/hgsubversion/verify.py not using absolute_import
  hgsubversion/hgsubversion/wrappers.py not using absolute_import
  hgsubversion/setup.py not using absolute_import
  hgsubversion/tests/comprehensive/test_custom_layout.py not using absolute_import
  hgsubversion/tests/comprehensive/test_obsstore_on.py not using absolute_import
  hgsubversion/tests/comprehensive/test_rebuildmeta.py not using absolute_import
  hgsubversion/tests/comprehensive/test_sqlite_revmap.py not using absolute_import
  hgsubversion/tests/comprehensive/test_stupid_pull.py not using absolute_import
  hgsubversion/tests/comprehensive/test_updatemeta.py not using absolute_import
  hgsubversion/tests/comprehensive/test_verify_and_startrev.py not using absolute_import
  hgsubversion/tests/fixtures/rsvn.py not using absolute_import
  hgsubversion/tests/fixtures/rsvn.py requires print_function
  hgsubversion/tests/run.py not using absolute_import
  hgsubversion/tests/test_binaryfiles.py not using absolute_import
  hgsubversion/tests/test_diff.py not using absolute_import
  hgsubversion/tests/test_externals.py not using absolute_import
  hgsubversion/tests/test_externals.py requires print_function
  hgsubversion/tests/test_fetch_branches.py not using absolute_import
  hgsubversion/tests/test_fetch_command.py not using absolute_import
  hgsubversion/tests/test_fetch_command_regexes.py not using absolute_import
  hgsubversion/tests/test_fetch_dir_removal.py not using absolute_import
  hgsubversion/tests/test_fetch_exec.py not using absolute_import
  hgsubversion/tests/test_fetch_mappings.py not using absolute_import
  hgsubversion/tests/test_fetch_renames.py not using absolute_import
  hgsubversion/tests/test_fetch_symlinks.py not using absolute_import
  hgsubversion/tests/test_fetch_truncated.py not using absolute_import
  hgsubversion/tests/test_helpers.py not using absolute_import
  hgsubversion/tests/test_hooks.py not using absolute_import
  hgsubversion/tests/test_pull.py not using absolute_import
  hgsubversion/tests/test_pull_fallback.py not using absolute_import
  hgsubversion/tests/test_push_autoprops.py not using absolute_import
  hgsubversion/tests/test_push_command.py not using absolute_import
  hgsubversion/tests/test_push_dirs.py not using absolute_import
  hgsubversion/tests/test_push_eol.py not using absolute_import
  hgsubversion/tests/test_push_renames.py not using absolute_import
  hgsubversion/tests/test_revmap_migrate.py not using absolute_import
  hgsubversion/tests/test_single_dir_clone.py not using absolute_import
  hgsubversion/tests/test_single_dir_clone.py requires print_function
  hgsubversion/tests/test_single_dir_push.py not using absolute_import
  hgsubversion/tests/test_svn_pre_commit_hooks.py not using absolute_import
  hgsubversion/tests/test_svnwrap.py not using absolute_import
  hgsubversion/tests/test_tags.py not using absolute_import
  hgsubversion/tests/test_template_keywords.py not using absolute_import
  hgsubversion/tests/test_template_keywords.py requires print_function
  hgsubversion/tests/test_unaffected_core.py not using absolute_import
  hgsubversion/tests/test_urls.py not using absolute_import
  hgsubversion/tests/test_util.py not using absolute_import
  hgsubversion/tests/test_util.py requires print_function
  hgsubversion/tests/test_utility_commands.py not using absolute_import
  remotenames/setup.py not using absolute_import
  setup.py not using absolute_import
  tests/conduithttp.py not using absolute_import
  tests/dummyext1.py not using absolute_import
  tests/dummyext2.py not using absolute_import
  tests/hggit/commitextra.py not using absolute_import
  tests/test-fb-hgext-extutil.py not using absolute_import
  tests/test-fb-hgext-fastmanifest.py not using absolute_import
  tests/test-fb-hgext-generic-bisect.py not using absolute_import
  tests/test-fb-hgext-patchpython.py not using absolute_import
  tests/test-fb-hgext-sshaskpass.py not using absolute_import
  tests/test-hggit-url-parsing.py not using absolute_import
  tests/test-hggit-url-parsing.py requires print_function
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
