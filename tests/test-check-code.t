#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg locate \
  > -X contrib/python-zstandard \
  > -X hgext/fsmonitor/pywatchman \
  > -X mercurial/thirdparty \
  > -X fb-hgext \
  > -X hg-git \
  > | sed 's-\\-/-g' | "$check_code" --warnings --per-file=0 - || false
  Skipping hgsql/hgsql.py it has no-che?k-code (glob)
  Skipping hgsql/tests/heredoctest.py it has no-che?k-code (glob)
  Skipping hgsql/tests/killdaemons.py it has no-che?k-code (glob)
  Skipping hgsql/tests/run-tests.py.old it has no-che?k-code (glob)
  Skipping hgsql/tests/test-encoding.t it has no-che?k-code (glob)
  Skipping hgsql/tests/test-race-conditions.t it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/__init__.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/compathacks.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/editor.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/hooks/updatemeta.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/layouts/base.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/layouts/custom.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/layouts/standard.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/maps.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/pushmod.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/stupid.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svncommands.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnexternals.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnmeta.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnrepo.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnwrap/__init__.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnwrap/common.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnwrap/subvertpy_wrapper.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/svnwrap/svn_swig_wrapper.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/util.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/verify.py it has no-che?k-code (glob)
  Skipping hgsubversion/hgsubversion/wrappers.py it has no-che?k-code (glob)
  Skipping hgsubversion/setup.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_custom_layout.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_obsstore_on.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_rebuildmeta.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_sqlite_revmap.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_stupid_pull.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_updatemeta.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/comprehensive/test_verify_and_startrev.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/fixtures/rsvn.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/run.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_externals.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_branches.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_command.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_command_regexes.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_exec.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_mappings.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_fetch_symlinks.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_push_command.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_push_dirs.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_push_renames.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_single_dir_clone.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_single_dir_push.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_svn_pre_commit_hooks.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_svnwrap.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_tags.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_template_keywords.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_urls.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_util.py it has no-che?k-code (glob)
  Skipping hgsubversion/tests/test_utility_commands.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping mercurial/statprof.py it has no-che?k-code (glob)
  Skipping tests/badserverext.py it has no-che?k-code (glob)
  tests/test-remotenames-basic.t:308:
   >   $ hg help bookmarks  | grep -A 3 -- '--track'
   don't use grep's context flags
  [1]

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('mercurial/debugcommands.py', 'rb') as fh:
  ...     for line in fh:
  ...         m = re.match("^@command\('([a-z]+)", line)
  ...         if m:
  ...             commands.append(m.group(1))
  >>> scommands = list(sorted(commands))
  >>> for i, command in enumerate(scommands):
  ...     if command != commands[i]:
  ...         print('commands in debugcommands.py not sorted; first differing '
  ...               'command is %s; expected %s' % (commands[i], command))
  ...         break

Prevent adding new files in the root directory accidentally.

  $ testrepohg files 'glob:*'
  .arcconfig
  .clang-format
  .editorconfig
  .hgignore
  .hgsigs
  .hgtags
  .jshintrc
  CONTRIBUTING
  CONTRIBUTORS
  COPYING
  Makefile
  README.rst
  hg
  hgeditor
  hgweb.cgi
  setup.py
