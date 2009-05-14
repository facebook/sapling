import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import test_binaryfiles
import test_diff
import test_externals
import test_fetch_branches
import test_fetch_command
import test_fetch_command_regexes
import test_fetch_exec
import test_fetch_mappings
import test_fetch_renames
import test_fetch_symlinks
import test_fetch_truncated
import test_pull
import test_push_command
import test_push_renames
import test_push_dirs
import test_push_eol
import test_rebuildmeta
import test_tags
import test_utility_commands
import test_urls

def suite():
    return unittest.TestSuite([test_binaryfiles.suite(),
                               test_diff.suite(),
                               test_externals.suite(),
                               test_fetch_branches.suite(),
                               test_fetch_command.suite(),
                               test_fetch_command_regexes.suite(),
                               test_fetch_exec.suite(),
                               test_fetch_mappings.suite(),
                               test_fetch_renames.suite(),
                               test_fetch_symlinks.suite(),
                               test_fetch_truncated.suite(),
                               test_pull.suite(),
                               test_push_command.suite(),
                               test_push_renames.suite(),
                               test_push_dirs.suite(),
                               test_push_eol.suite(),
                               test_rebuildmeta.suite(),
                               test_tags.suite(),
                               test_utility_commands.suite(),
                               test_urls.suite(),
                              ])

if __name__ == '__main__':
    kwargs = { 'descriptions': 2 }
    if '-v' in sys.argv:
        kwargs['descriptions'] = 3
        kwargs['verbosity'] = 2

    runner = unittest.TextTestRunner(**kwargs)
    runner.run(suite())
