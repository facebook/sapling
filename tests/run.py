import os
import sys
import unittest

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import test_fetch_command
import test_fetch_command_regexes
import test_fetch_renames
import test_fetch_truncated
import test_push_command
import test_push_renames
import test_push_dirs
import test_push_eol
import test_tags

def suite():
    return unittest.TestSuite([test_fetch_command.suite(),
                               test_fetch_command_regexes.suite(),
                               test_fetch_renames.suite(),
                               test_fetch_truncated.suite(),
                               test_push_command.suite(),
                               test_push_renames.suite(),
                               test_push_dirs.suite(),
                               test_push_eol.suite(),
                               test_tags.suite(),
                              ])

if __name__ == '__main__':
    runner = unittest.TextTestRunner()
    runner.run(suite())
