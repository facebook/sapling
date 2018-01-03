#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

  $ testrepohg files 'set:(**.py)' \
  > -X hgdemandimport/demandimportpy2.py \
  > -X hg-git \
  > | sed 's|\\|/|g' | xargs $PYTHON contrib/check-py3-compat.py
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
  fb-hgext/fastmanifest/__init__.py not using absolute_import
  fb-hgext/fastmanifest/cachemanager.py not using absolute_import
  fb-hgext/fastmanifest/concurrency.py not using absolute_import
  fb-hgext/fastmanifest/constants.py not using absolute_import
  fb-hgext/fastmanifest/debug.py not using absolute_import
  fb-hgext/fastmanifest/implementation.py not using absolute_import
  fb-hgext/fastmanifest/metrics.py not using absolute_import
  fb-hgext/hgext3rd/arcdiff.py not using absolute_import
  fb-hgext/hgext3rd/backups.py not using absolute_import
  fb-hgext/hgext3rd/catnotate.py not using absolute_import
  fb-hgext/hgext3rd/checkmessagehook.py not using absolute_import
  fb-hgext/hgext3rd/chistedit.py not using absolute_import
  fb-hgext/hgext3rd/copytrace.py not using absolute_import
  fb-hgext/hgext3rd/debugcommitmessage.py not using absolute_import
  fb-hgext/hgext3rd/dialect.py not using absolute_import
  fb-hgext/hgext3rd/directaccess.py not using absolute_import
  fb-hgext/hgext3rd/drop.py not using absolute_import
  fb-hgext/hgext3rd/edrecord.py not using absolute_import
  fb-hgext/hgext3rd/errorredirect.py not using absolute_import
  fb-hgext/hgext3rd/extorder.py not using absolute_import
  fb-hgext/hgext3rd/fastannotate/error.py not using absolute_import
  fb-hgext/hgext3rd/fastannotate/formatter.py not using absolute_import
  fb-hgext/hgext3rd/fastannotate/protocol.py not using absolute_import
  fb-hgext/hgext3rd/fastlog.py not using absolute_import
  fb-hgext/hgext3rd/fastpartialmatch.py not using absolute_import
  fb-hgext/hgext3rd/fbconduit.py not using absolute_import
  fb-hgext/hgext3rd/fbhistedit.py not using absolute_import
  fb-hgext/hgext3rd/fbshow.py not using absolute_import
  fb-hgext/hgext3rd/fbsparse.py not using absolute_import
  fb-hgext/hgext3rd/generic_bisect.py not using absolute_import
  fb-hgext/hgext3rd/githelp.py not using absolute_import
  fb-hgext/hgext3rd/gitlookup.py not using absolute_import
  fb-hgext/hgext3rd/grepdiff.py not using absolute_import
  fb-hgext/hgext3rd/grpcheck.py not using absolute_import
  fb-hgext/hgext3rd/linkrevcache.py not using absolute_import
  fb-hgext/hgext3rd/logginghelper.py not using absolute_import
  fb-hgext/hgext3rd/morestatus.py not using absolute_import
  fb-hgext/hgext3rd/myparent.py not using absolute_import
  fb-hgext/hgext3rd/nointerrupt.py not using absolute_import
  fb-hgext/hgext3rd/ownercheck.py not using absolute_import
  fb-hgext/hgext3rd/p4fastimport/filetransaction.py not using absolute_import
  fb-hgext/hgext3rd/patchpython.py not using absolute_import
  fb-hgext/hgext3rd/perftweaks.py not using absolute_import
  fb-hgext/hgext3rd/phabdiff.py not using absolute_import
  fb-hgext/hgext3rd/phabstatus.py not using absolute_import
  fb-hgext/hgext3rd/phrevset.py not using absolute_import
  fb-hgext/hgext3rd/pullcreatemarkers.py not using absolute_import
  fb-hgext/hgext3rd/rage.py not using absolute_import
  fb-hgext/hgext3rd/remoteid.py not using absolute_import
  fb-hgext/hgext3rd/reset.py not using absolute_import
  fb-hgext/hgext3rd/sampling.py not using absolute_import
  fb-hgext/hgext3rd/sigtrace.py not using absolute_import
  fb-hgext/hgext3rd/simplecache.py not using absolute_import
  fb-hgext/hgext3rd/sparse.py not using absolute_import
  fb-hgext/hgext3rd/sshaskpass.py not using absolute_import
  fb-hgext/hgext3rd/stat.py not using absolute_import
  fb-hgext/hgext3rd/upgradegeneraldelta.py not using absolute_import
  fb-hgext/hgext3rd/whereami.py not using absolute_import
  fb-hgext/infinitepush/bundleparts.py not using absolute_import
  fb-hgext/infinitepush/common.py not using absolute_import
  fb-hgext/infinitepush/fileindexapi.py not using absolute_import
  fb-hgext/infinitepush/indexapi.py not using absolute_import
  fb-hgext/infinitepush/sqlindexapi.py not using absolute_import
  fb-hgext/infinitepush/store.py not using absolute_import
  fb-hgext/linelog/pyext/test-random-edits.py not using absolute_import
  fb-hgext/phabricator/arcconfig.py not using absolute_import
  fb-hgext/phabricator/diffprops.py not using absolute_import
  fb-hgext/phabricator/graphql.py not using absolute_import
  fb-hgext/phabricator/phabricator_graphql_client_requests.py not using absolute_import
  fb-hgext/phabricator/phabricator_graphql_client_urllib.py not using absolute_import
  fb-hgext/remotefilelog/__init__.py not using absolute_import
  fb-hgext/remotefilelog/cacheclient.py not using absolute_import
  fb-hgext/remotefilelog/constants.py not using absolute_import
  fb-hgext/remotefilelog/historypack.py not using absolute_import
  fb-hgext/remotefilelog/lz4wrapper.py not using absolute_import
  fb-hgext/remotefilelog/metadatastore.py not using absolute_import
  fb-hgext/remotefilelog/remotefilectx.py not using absolute_import
  fb-hgext/remotefilelog/shallowbundle.py not using absolute_import
  fb-hgext/remotefilelog/shallowrepo.py not using absolute_import
  fb-hgext/remotefilelog/shallowstore.py not using absolute_import
  fb-hgext/remotefilelog/shallowverifier.py not using absolute_import
  fb-hgext/remotefilelog/wirepack.py not using absolute_import
  fb-hgext/scripts/lint.py not using absolute_import
  fb-hgext/scripts/lint.py requires print_function
  fb-hgext/scripts/unit.py not using absolute_import
  fb-hgext/scripts/utils.py not using absolute_import
  fb-hgext/setup.py not using absolute_import
  fb-hgext/tests/bundlerepologger.py not using absolute_import
  fb-hgext/tests/conduithttp.py not using absolute_import
  fb-hgext/tests/dummyext1.py not using absolute_import
  fb-hgext/tests/dummyext2.py not using absolute_import
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
  fb-hgext/tests/test-absorb-filefixupstate.py not using absolute_import
  fb-hgext/tests/test-extutil.py not using absolute_import
  fb-hgext/tests/test-fastmanifest.py not using absolute_import
  fb-hgext/tests/test-generic-bisect.py not using absolute_import
  fb-hgext/tests/test-patchpython.py not using absolute_import
  fb-hgext/tests/test-sshaskpass.py not using absolute_import
  fb-hgext/tests/treemanifest_correctness.py not using absolute_import
  fb-hgext/tests/waitforfile.py not using absolute_import
  fb-hgext/treemanifest/__init__.py not using absolute_import
  hgext/remotenames.py not using absolute_import
  remotenames/setup.py not using absolute_import
  setup.py not using absolute_import

#if py3exe
  $ testrepohg files 'set:(**.py) - grep(pygments)' \
  > -X hgdemandimport/demandimportpy2.py \
  > -X hgext/fsmonitor/pywatchman \
  > | sed 's|\\|/|g' | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
  hgext/convert/transport.py: error importing: <*Error> No module named 'svn.client' (error at transport.py:*) (glob)
  mercurial/cffi/bdiff.py: error importing: <ImportError> cannot import name '_bdiff' (error at bdiff.py:*)
  mercurial/cffi/bdiffbuild.py: error importing: <ImportError> No module named 'cffi' (error at bdiffbuild.py:*)
  mercurial/cffi/mpatch.py: error importing: <ImportError> cannot import name '_mpatch' (error at mpatch.py:*)
  mercurial/cffi/mpatchbuild.py: error importing: <ImportError> No module named 'cffi' (error at mpatchbuild.py:*)
  mercurial/cffi/osutilbuild.py: error importing: <ImportError> No module named 'cffi' (error at osutilbuild.py:*)
  mercurial/scmwindows.py: error importing: <*Error> No module named 'msvcrt' (error at win32.py:*) (glob)
  mercurial/win32.py: error importing: <*Error> No module named 'msvcrt' (error at win32.py:*) (glob)
  mercurial/windows.py: error importing: <*Error> No module named 'msvcrt' (error at windows.py:*) (glob)

#endif

#if py3exe py3pygments
  $ testrepohg files 'set:(**.py) and grep(pygments)' | sed 's|\\|/|g' \
  > | xargs $PYTHON3 contrib/check-py3-compat.py \
  > | sed 's/[0-9][0-9]*)$/*)/'
#endif
