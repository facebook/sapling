import unittest

import test_util

from hgsubversion import stupid


two_empties = """Index: __init__.py
===================================================================
Index: bar/__init__.py
===================================================================
Index: bar/test_muhaha.py
===================================================================
--- bar/test_muhaha.py	(revision 0)
+++ bar/test_muhaha.py	(revision 1)
@@ -0,0 +1,2 @@
+
+blah blah blah, I'm a fake patch
\ No newline at end of file
"""

binary_delta = """Index: trunk/functional_tests/doc_tests/test_doctest_fixtures/doctest_fixtures_fixtures.pyc
===================================================================
Cannot display: file marked as a binary type.
svn:mime-type = application/octet-stream

Property changes on: trunk/functional_tests/doc_tests/test_doctest_fixtures/doctest_fixtures_fixtures.pyc
___________________________________________________________________
Added: svn:mime-type
   + application/octet-stream

Index: trunk/functional_tests/doc_tests/test_doctest_fixtures/doctest_fixtures.rst
===================================================================
"""

special_delta = """Index: delta
===================================================================
--- delta(revision 0)
+++ delta(revision 9)
@@ -0,0 +1 @@
+link alpha
\ No newline at end of file

Property changes on: delta
___________________________________________________________________
Name: svn:special
   + *

"""

class RegexTests(unittest.TestCase):
    def test_empty_file_re(self):
        changed = stupid.parsediff(two_empties)
        self.assertEqual(3, len(changed))
        self.assertEqual('__init__.py', changed[0].name)
        self.assert_(changed[0].isempty())
        self.assertEqual('bar/__init__.py', changed[1].name)
        self.assert_(changed[1].isempty())
        self.assertEqual('bar/test_muhaha.py', changed[2].name)
        self.assert_(not changed[2].isempty())

    def test_any_matches_just_one(self):
        pat = '''Index: trunk/django/contrib/admin/urls/__init__.py
===================================================================
'''
        changed = stupid.parsediff(pat)
        self.assertEqual(['trunk/django/contrib/admin/urls/__init__.py'],
                         [f.name for f in changed])

    def test_special_re(self):
        changed = stupid.parsediff(special_delta)
        self.assertEqual(1, len(changed))
        self.assert_(changed[0].symlink)

    def test_any_file_re(self):
        changed = stupid.parsediff(two_empties)
        self.assertEqual(['__init__.py', 'bar/__init__.py', 'bar/test_muhaha.py'],
                         [f.name for f in changed])

    def test_binary_file_re(self):
        changed = stupid.parsediff(binary_delta)
        binaries = [f.name for f in changed if f.binary]
        self.assertEqual(['trunk/functional_tests/doc_tests/test_doctest_fixtures/doctest_fixtures_fixtures.pyc'],
                         binaries)

    def test_diff16(self):
        data = """Index: d3/d
===================================================================
--- d3/d        (revision 0)
+++ d3/d        (revision 6)
@@ -0,0 +1 @@
+d

Property changes on: d3
___________________________________________________________________
Added: svn:externals
   + ^/trunk/common/ext ext3



Property changes on: .
___________________________________________________________________
Added: svn:mergeinfo
   Merged /branches/branch:r4-5
"""
        changed = stupid.parsediff(data)
        self.assertEqual(['d3/d', 'd3', '.'], [f.name for f in changed])
        data = """Property changes on: empty1
___________________________________________________________________
Deleted: svn:executable
   - *


Property changes on: empty2
___________________________________________________________________
Added: svn:executable
   + *


Property changes on: binary1
___________________________________________________________________
Deleted: svn:executable
   - *


Property changes on: text1
___________________________________________________________________
Deleted: svn:executable
   - *


Property changes on: binary2
___________________________________________________________________
Added: svn:executable
   + *


Property changes on: text2
___________________________________________________________________
Added: svn:executable
   + *
"""
        changed = stupid.parsediff(data)
        self.assertEqual(['empty1', 'empty2', 'binary1', 'text1', 'binary2', 'text2'],
                         [f.name for f in changed])
