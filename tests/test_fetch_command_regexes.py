import fetch_command

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

def test_empty_file_re():
    matches = fetch_command.empty_file_patch_wont_make_re.findall(two_empties)
    assert sorted(matches) == ['__init__.py', 'bar/__init__.py']

def test_any_matches_just_one():
    pat = '''Index: trunk/django/contrib/admin/urls/__init__.py
===================================================================
'''
    matches = fetch_command.any_file_re.findall(pat)
    assert len(matches) == 1

def test_any_file_re():
    matches = fetch_command.any_file_re.findall(two_empties)
    assert sorted(matches) == ['__init__.py', 'bar/__init__.py',
                               'bar/test_muhaha.py']
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
def test_binary_file_re():
    matches = fetch_command.binary_file_re.findall(binary_delta)
    print matches
    assert matches == ['trunk/functional_tests/doc_tests/test_doctest_fixtures/doctest_fixtures_fixtures.pyc']
