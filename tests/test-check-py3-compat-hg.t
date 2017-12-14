#require test-repo

  $ . $TESTDIR/require-core-hg.sh contrib/check-py3-compat.py

This file is backported from mercurial/tests/test-check-py3-compat.t.

  $ . "$TESTDIR/helper-testrepo.sh"

  $ cd "$TESTDIR"/..
  $ hg files 'set:(**.py)' | sed 's|\\|/|g' | xargs $PYTHON $RUNTESTDIR/../contrib/check-py3-compat.py
  fastmanifest/__init__.py not using absolute_import
  fastmanifest/cachemanager.py not using absolute_import
  fastmanifest/concurrency.py not using absolute_import
  fastmanifest/constants.py not using absolute_import
  fastmanifest/debug.py not using absolute_import
  fastmanifest/implementation.py not using absolute_import
  fastmanifest/metrics.py not using absolute_import
  hgext3rd/arcdiff.py not using absolute_import
  hgext3rd/backups.py not using absolute_import
  hgext3rd/catnotate.py not using absolute_import
  hgext3rd/checkmessagehook.py not using absolute_import
  hgext3rd/chistedit.py not using absolute_import
  hgext3rd/copytrace.py not using absolute_import
  hgext3rd/debugcommitmessage.py not using absolute_import
  hgext3rd/dialect.py not using absolute_import
  hgext3rd/directaccess.py not using absolute_import
  hgext3rd/drop.py not using absolute_import
  hgext3rd/edrecord.py not using absolute_import
  hgext3rd/errorredirect.py not using absolute_import
  hgext3rd/extorder.py not using absolute_import
  hgext3rd/fastannotate/error.py not using absolute_import
  hgext3rd/fastannotate/formatter.py not using absolute_import
  hgext3rd/fastannotate/protocol.py not using absolute_import
  hgext3rd/fastlog.py not using absolute_import
  hgext3rd/fastpartialmatch.py not using absolute_import
  hgext3rd/fbconduit.py not using absolute_import
  hgext3rd/fbhistedit.py not using absolute_import
  hgext3rd/fbshow.py not using absolute_import
  hgext3rd/fbsparse.py not using absolute_import
  hgext3rd/generic_bisect.py not using absolute_import
  hgext3rd/githelp.py not using absolute_import
  hgext3rd/gitlookup.py not using absolute_import
  hgext3rd/grepdiff.py not using absolute_import
  hgext3rd/grpcheck.py not using absolute_import
  hgext3rd/linkrevcache.py not using absolute_import
  hgext3rd/logginghelper.py not using absolute_import
  hgext3rd/morestatus.py not using absolute_import
  hgext3rd/myparent.py not using absolute_import
  hgext3rd/nointerrupt.py not using absolute_import
  hgext3rd/ownercheck.py not using absolute_import
  hgext3rd/p4fastimport/filetransaction.py not using absolute_import
  hgext3rd/patchpython.py not using absolute_import
  hgext3rd/perftweaks.py not using absolute_import
  hgext3rd/phabdiff.py not using absolute_import
  hgext3rd/phabstatus.py not using absolute_import
  hgext3rd/phrevset.py not using absolute_import
  hgext3rd/pullcreatemarkers.py not using absolute_import
  hgext3rd/rage.py not using absolute_import
  hgext3rd/remoteid.py not using absolute_import
  hgext3rd/reset.py not using absolute_import
  hgext3rd/sampling.py not using absolute_import
  hgext3rd/sigtrace.py not using absolute_import
  hgext3rd/simplecache.py not using absolute_import
  hgext3rd/sparse.py not using absolute_import
  hgext3rd/sshaskpass.py not using absolute_import
  hgext3rd/stat.py not using absolute_import
  hgext3rd/upgradegeneraldelta.py not using absolute_import
  hgext3rd/whereami.py not using absolute_import
  infinitepush/bundleparts.py not using absolute_import
  infinitepush/common.py not using absolute_import
  infinitepush/fileindexapi.py not using absolute_import
  infinitepush/indexapi.py not using absolute_import
  infinitepush/sqlindexapi.py not using absolute_import
  infinitepush/store.py not using absolute_import
  linelog/pyext/test-random-edits.py not using absolute_import
  phabricator/arcconfig.py not using absolute_import
  phabricator/diffprops.py not using absolute_import
  phabricator/graphql.py not using absolute_import
  phabricator/phabricator_graphql_client_requests.py not using absolute_import
  phabricator/phabricator_graphql_client_urllib.py not using absolute_import
  remotefilelog/__init__.py not using absolute_import
  remotefilelog/cacheclient.py not using absolute_import
  remotefilelog/constants.py not using absolute_import
  remotefilelog/historypack.py not using absolute_import
  remotefilelog/lz4wrapper.py not using absolute_import
  remotefilelog/metadatastore.py not using absolute_import
  remotefilelog/remotefilectx.py not using absolute_import
  remotefilelog/shallowbundle.py not using absolute_import
  remotefilelog/shallowrepo.py not using absolute_import
  remotefilelog/shallowstore.py not using absolute_import
  remotefilelog/shallowverifier.py not using absolute_import
  remotefilelog/wirepack.py not using absolute_import
  scripts/lint.py not using absolute_import
  scripts/lint.py requires print_function
  scripts/unit.py not using absolute_import
  scripts/utils.py not using absolute_import
  setup.py not using absolute_import
  tests/bundlerepologger.py not using absolute_import
  tests/conduithttp.py not using absolute_import
  tests/dummyext1.py not using absolute_import
  tests/dummyext2.py not using absolute_import
  tests/get-with-headers.py not using absolute_import
  tests/get-with-headers.py requires print_function
  tests/getflogheads.py not using absolute_import
  tests/heredoctest.py not using absolute_import
  tests/heredoctest.py requires print_function
  tests/killdaemons.py not using absolute_import
  tests/ls-l.py not using absolute_import
  tests/ls-l.py requires print_function
  tests/perftest.py not using absolute_import
  tests/perftest.py requires print_function
  tests/test-absorb-filefixupstate.py not using absolute_import
  tests/test-extutil.py not using absolute_import
  tests/test-fastmanifest.py not using absolute_import
  tests/test-generic-bisect.py not using absolute_import
  tests/test-patchpython.py not using absolute_import
  tests/test-sshaskpass.py not using absolute_import
  tests/treemanifest_correctness.py not using absolute_import
  tests/waitforfile.py not using absolute_import
  treemanifest/__init__.py not using absolute_import
