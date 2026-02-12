# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

from unittest import TestCase

from eden.mononoke.tests.mock_restricted_path.test_infra.python.simple.simple import add


class UnittestTest(TestCase):
    # Simple tests to demonstrate unittest features
    def test_upper(self) -> None:
        self.assertEqual("foo".upper(), "FOO")

    def test_isupper(self):
        self.assertTrue("FOO".isupper())
        self.assertFalse("Foo".isupper())

    def test_split(self):
        s = "hello world"
        self.assertEqual(s.split(), ["hello", "world"])
        # check that s.split fails when the separator is not a string
        with self.assertRaises(TypeError):
            s.split(2)

    # Compare two lists in different order
    def test_count_equal(self):
        self.assertNotEqual([1, 2, 2, 3], [3, 2, 2, 1])
        self.assertCountEqual([1, 2, 2, 3], [3, 2, 2, 1])

    # Actually testing the imported function
    def test_add_simple(self) -> None:
        self.assertEqual(add(2, 2), 4)

    # Parameterized test
    def test_add_parameterized(self) -> None:
        add_parameters = [(1, 2, 3), (4, 2, 6), (15, 17, 32)]
        for a, b, expected in add_parameters:
            # Using subTest is better than just using assertEqual in a loop
            # because it will try all the parameter tuples instead of stopping
            # on the first failure
            with self.subTest(a=a, b=b, expected=expected):
                self.assertEqual(add(a, b), expected)


class UnittestWithFixtureTest(TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        print("Setting up UnittestWithFixtureTest class")
        cls.num1 = 10
        cls.num2 = 20
        cls.list_of_numbers = [1, 2, 3]

    def test_add_numbers(self) -> None:
        result = self.num1 + self.num2
        self.assertEqual(result, 30)

    def test_check_and_modify_numbers_list(self) -> None:
        self.list_of_numbers.append(4)
        self.assertEqual(self.list_of_numbers, [1, 2, 3, 4])

    def test_check_numbers_list_is_not_modified(self) -> None:
        # Even though the list is modified in the previous test, it should still
        # be the same as the original value
        self.assertEqual(self.list_of_numbers, [1, 2, 3])


# tests using parameterized
from parameterized import parameterized


class UnittestWithParameterizedTest(TestCase):
    @parameterized.expand(
        [
            (1, 2, 3),
            (4, 2, 6),
            (15, 17, 32),
        ]
    )
    def test_add_parameterized(self, a, b, expected) -> None:
        self.assertEqual(add(a, b), expected)
