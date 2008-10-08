import os
import sys
import unittest

sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import test_fetch_command
import test_fetch_command_regexes
import test_push_command

def suite():
    return unittest.TestSuite([test_fetch_command.suite(),
                               test_fetch_command_regexes.suite(),
                               test_fetch_command_regexes.suite(),
                              ])

if __name__ == '__main__':
    runner = unittest.TextTestRunner()
    runner.run(suite())
