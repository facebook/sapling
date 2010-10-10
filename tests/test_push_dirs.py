import test_util

import unittest

class TestPushDirectories(test_util.TestBase):
    def test_push_dirs(self):
        self._load_fixture_and_fetch('emptyrepo.svndump')

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
                         ['d2', 'd2/b', 'd31', 'd31/d32', 'd31/d32/a', ])

    def test_push_new_dir_project_root_not_repo_root(self):
        self._load_fixture_and_fetch('fetch_missing_files_subdir.svndump',
                                     subdir='foo')
        changes = [('magic_new/a', 'magic_new/a', 'ohai', ),
                   ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('foo/trunk'), ['bar',
                                                   'bar/alpha',
                                                   'bar/beta',
                                                   'bar/delta',
                                                   'bar/gamma',
                                                   'foo',
                                                   'magic_new',
                                                   'magic_new/a'])

    def test_push_new_file_existing_dir_root_not_repo_root(self):
        self._load_fixture_and_fetch('empty_dir_in_trunk_not_repo_root.svndump',
                                     subdir='project')
        changes = [('narf/a', 'narf/a', 'ohai', ),
                   ]
        self.commitchanges(changes)
        self.assertEqual(self.svnls('project/trunk'), ['a',
                                                       'narf',])
        self.pushrevisions()
        self.assertEqual(self.svnls('project/trunk'), ['a',
                                                       'narf',
                                                       'narf/a'])
        changes = [('narf/a', None, None, ),
                   ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('project/trunk'), ['a' ,])

    def test_push_single_dir_change_in_subdir(self):
        # Tests simple pushing from default branch to a single dir repo
        # Changes a file in a subdir (regression).
        repo = self._load_fixture_and_fetch('branch_from_tag.svndump',
                                            stupid=False,
                                            layout='single',
                                            subdir='tags')
        changes = [('tag_r3/alpha', 'tag_r3/alpha', 'foo'),
                   ('tag_r3/new', 'tag_r3/new', 'foo'),
                   ('new_dir/new', 'new_dir/new', 'foo'),
                   ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('tags'),
                         ['copied_tag',
                          'copied_tag/alpha',
                          'copied_tag/beta',
                          'new_dir',
                          'new_dir/new',
                          'tag_r3',
                          'tag_r3/alpha',
                          'tag_r3/beta',
                          'tag_r3/new'])

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestPushDirectories),
          ]
    return unittest.TestSuite(all)
