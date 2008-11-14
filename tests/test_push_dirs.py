import unittest

import test_util

class TestPushDirectories(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        test_util.load_fixture_and_fetch('emptyrepo.svndump',
                                         self.repo_path,
                                         self.wc_path)

    def test_push_dirs(self):
        changes = [
            # Single file in single directory
            ('d1/a', 'd1/a', 'a\n'),
            # Two files in one directory
            ('d2/a', 'd2/a', 'a\n'),
            ('d2/b', 'd2/b', 'a\n'),
            # Single file in empty directory hierarchy
            ('d31/d32/d33/d34/a', 'd31/d32/d33/d34/a', 'a\n'),
            ('d31/d32/a', 'd31/d32/a', 'a\n'),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('trunk'), 
                          ['d1', 'd1/a', 'd2', 'd2/a', 'd2/b', 'd31', 
                           'd31/d32', 'd31/d32/a', 'd31/d32/d33', 
                           'd31/d32/d33/d34', 'd31/d32/d33/d34/a'])

        # Add one revision with changed files only, no directory addition
        # or deletion.
        changes = [
            ('d1/a', 'd1/a', 'aa\n'),
            ('d2/a', 'd2/a', 'aa\n'),
            ]
        self.commitchanges(changes)
        self.pushrevisions()

        changes = [
            # Remove single file in single directory
            ('d1/a', None, None),
            # Remove one file out of two
            ('d2/a', None, None),
            # Removing this file should remove one empty parent dir too
            ('d31/d32/d33/d34/a', None, None),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('trunk'), 
                         ['d2', 'd2/b', 'd31', 'd31/d32', 'd31/d32/a', 'd31/d32/d33'])

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestPushDirectories),
          ]
    return unittest.TestSuite(all)
