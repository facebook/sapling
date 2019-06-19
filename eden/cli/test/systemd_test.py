#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import collections
import pathlib
import typing
import unittest

import hypothesis
import hypothesis.strategies
from eden.cli.systemd import (
    SystemdEnvironmentFile,
    escape_dbus_address,
    systemd_escape_path,
)
from eden.test_support.hypothesis import fast_hypothesis_test, set_up_hypothesis


set_up_hypothesis()


class SystemdEscapeTest(unittest.TestCase):
    def test_escape_benign_absolute_path(self) -> None:
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath("/path/to/file.txt")),
            "path-to-file.txt",
        )

    def test_escape_path_containing_funky_characters(self) -> None:
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath("/file with spaces")),
            r"file\x20with\x20spaces",
        )
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath(r"/file\with\backslashes")),
            r"file\x5cwith\x5cbackslashes",
        )
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath(r"/Hallöchen, Meister")),
            r"Hall\xc3\xb6chen\x2c\x20Meister",
        )

    def test_escape_path_containing_newlines(self) -> None:
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath("/file\nwith\nnewlines")),
            r"file\x0awith\x0anewlines",
        )
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath("/trailing\n")), r"trailing\x0a"
        )
        self.assertEqual(systemd_escape_path(pathlib.PurePosixPath("/\n")), r"\x0a")

    def test_escaping_path_ignores_trailing_slashes(self) -> None:
        self.assertEqual(
            systemd_escape_path(pathlib.PurePosixPath("/path/to/directory///")),
            "path-to-directory",
        )

    def test_escape_relative_path_raises(self) -> None:
        path = pathlib.PurePosixPath("path/to/file.txt")
        with self.assertRaises(ValueError):
            systemd_escape_path(path)

    def test_escape_path_with_dotdot_components_raises(self) -> None:
        path = pathlib.PurePosixPath("/path/to/../file.txt")
        with self.assertRaises(ValueError):
            systemd_escape_path(path)


class SystemdEnvironmentFileDumpTest(unittest.TestCase):
    # TODO(strager): Reject variables whose values are not valid UTF-8.

    def test_file_with_no_variables_is_empty(self) -> None:
        self.assertEqual(self.dumps({}), b"")

    def test_load_dumped_single_simple_variable(self) -> None:
        self.assertEqual(
            self.dump_and_load({b"my_variable": b"my_value"}).entries,
            [(b"my_variable", b"my_value")],
        )
        self.assertEqual(
            self.dump_and_load({b"MESSAGE": b"hello"}).entries, [(b"MESSAGE", b"hello")]
        )

    def test_load_dumped_multiple_simple_variables(self) -> None:
        variables: typing.MutableMapping[bytes, bytes] = collections.OrderedDict()
        variables[b"var1"] = b"val1"
        variables[b"var2"] = b"val2"
        variables[b"var3"] = b"val3"
        self.assertEqual(
            self.dump_and_load(variables).entries,
            [(b"var1", b"val1"), (b"var2", b"val2"), (b"var3", b"val3")],
        )

    def test_empty_names_are_disallowed(self) -> None:
        variables = {b"": b"value"}
        with self.assertRaisesRegex(ValueError, "Variables must have a non-empty name"):
            self.dumps(variables)

    def test_leading_digit_is_disallowed_in_names(self) -> None:
        variables = {b"1up": b"value"}
        with self.assertRaisesRegex(
            ValueError, "Variable names must not begin with a digit"
        ):
            self.dumps(variables)

    def test_whitespace_is_disallowed_in_names(self) -> None:
        for name in [
            b" leading_space",
            b"trailing_space ",
            b"interior space",
            b"\tleading_tab",
            b"trailing_tab\t",
        ]:
            with self.subTest(name=name):
                variables = {name: b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain whitespace"
                ):
                    self.dumps(variables)

    def test_equal_sign_is_disallowed_in_names(self) -> None:
        for name in [b"hello=world", b"var=", b"=var"]:
            with self.subTest(name=name):
                variables = {name: b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain '='"
                ):
                    self.dumps(variables)

    def test_backslashes_are_disallowed_in_names(self) -> None:
        for name in [b"\\name", b"name\\", b"abc\\ def", b"abc\\\\def"]:
            with self.subTest(name=name):
                variables = {name: b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain '\\\\'"
                ):
                    self.dumps(variables)

    def test_newlines_are_disallowed_in_names(self) -> None:
        variables = {b"hello\nworld": b"value"}
        with self.assertRaisesRegex(
            ValueError, "Variable names must not contain any newline characters"
        ):
            self.dumps(variables)

    def test_symbols_are_disallowed_in_names(self) -> None:
        for symbol_byte in b"`~!@$%^&*()[]+-.,/?'\"\\|":
            symbol = bytes([symbol_byte])
            with self.subTest(symbol=symbol):
                variables = {b"hello" + symbol + b"world": b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain '.'"
                ):
                    self.dumps(variables)

    def test_comment_characters_are_disallowed_in_names(self) -> None:
        for name in [b"#name", b";name", b"name#", b"hello#world"]:
            with self.subTest(name=name):
                variables = {name: b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain '[#;]'"
                ):
                    self.dumps(variables)

    def test_control_characters_are_disallowed_in_names(self) -> None:
        for control_character_byte in b"\x00\x01\a\x1b":
            control_character = bytes([control_character_byte])
            with self.subTest(control_character=control_character):
                variables = {b"a" + control_character + b"b": b"value"}
                with self.assertRaisesRegex(
                    ValueError, "Variable names must not contain any control characters"
                ):
                    self.dumps(variables)

    def test_empty_values_are_allowed(self) -> None:
        self.assert_variable_dumps_and_loads(b"var", b"")

    def test_whitespace_is_allowed_in_values(self) -> None:
        self.assert_variable_dumps_and_loads(b"var", b" value with spaces ")
        self.assert_variable_dumps_and_loads(b"var", b"\tvalue\twith\ttabs\t")

    def test_backslashes_are_allowed_in_values(self) -> None:
        self.assert_variable_dumps_and_loads(b"var", b"\\value_with_backslashes\\")
        self.assert_variable_dumps_and_loads(b"var", b"\\n")
        self.assert_variable_dumps_and_loads(b"var", b"abc\\def")
        self.assert_variable_dumps_and_loads(b"var", b"abc\\\\def")

    def test_quotes_are_allowed_in_values(self) -> None:
        self.assert_variable_dumps_and_loads(b"var", b"'value_with_single_quotes'")
        self.assert_variable_dumps_and_loads(b"var", b'"value_with_double_quotes"')
        self.assert_variable_dumps_and_loads(b"var", b"that's all folks")
        self.assert_variable_dumps_and_loads(b"var", b'unterminated " double quote')
        self.assert_variable_dumps_and_loads(b"var", b"\"'\"'")

    def test_newlines_are_allowed_in_values(self) -> None:
        self.assert_variable_dumps_and_loads(b"var", b"abc\ndef")

    def test_carriage_returns_are_disallowed_in_values(self) -> None:
        variables = {b"name": b"abc\rdef"}
        with self.assertRaisesRegex(
            ValueError, "Variable values must not contain carriage returns"
        ):
            self.dumps(variables)

    def test_control_characters_are_disallowed_in_values(self) -> None:
        for control_character_byte in b"\x00\x01\a\x1b":
            control_character = bytes([control_character_byte])
            with self.subTest(control_character=control_character):
                variables = {b"name": b"abc" + control_character + b"def"}
                with self.assertRaisesRegex(
                    ValueError,
                    "Variable values must not contain any control characters",
                ):
                    self.dumps(variables)

    @fast_hypothesis_test()
    @hypothesis.given(hypothesis.strategies.binary())
    def test_arbitrary_variable_value_round_trips_through_dump_and_load(
        self, value: bytes
    ) -> None:
        self.hypothesis_assume_variable_value_is_valid(value)
        self.assert_variable_dumps_and_loads(b"var", value)

    @fast_hypothesis_test()
    @hypothesis.given(hypothesis.strategies.binary())
    def test_arbitrary_variable_name_round_trips_through_dump_and_load(
        self, name: bytes
    ) -> None:
        self.hypothesis_assume_variable_name_is_valid(name)
        self.assert_variable_dumps_and_loads(name, b"value")

    @fast_hypothesis_test()
    @hypothesis.given(hypothesis.strategies.binary(), hypothesis.strategies.binary())
    def test_arbitrary_variable_name_and_value_round_trips_through_dump_and_load(
        self, name: bytes, value: bytes
    ) -> None:
        self.hypothesis_assume_variable_name_is_valid(name)
        self.hypothesis_assume_variable_value_is_valid(value)
        self.assert_variable_dumps_and_loads(name, value)

    def assert_variable_dumps_and_loads(self, name: bytes, value: bytes) -> None:
        self.assertEqual(self.dump_and_load({name: value}).entries, [(name, value)])

    def dump_and_load(
        self, variables: typing.Mapping[bytes, bytes]
    ) -> SystemdEnvironmentFile:
        return self.loads(self.dumps(variables))

    def dumps(self, variables: typing.Mapping[bytes, bytes]) -> bytes:
        return SystemdEnvironmentFile.dumps(variables)

    def loads(self, content: bytes) -> SystemdEnvironmentFile:
        return SystemdEnvironmentFile.loads(content)

    def hypothesis_assume_variable_name_is_valid(self, name: bytes) -> None:
        lower_alphabet = b"abcdefghijklmnopqrstuvwxyz"
        upper_alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        digits = b"0123456789"
        allowed_characters = b"".join([b"_", lower_alphabet, upper_alphabet, digits])
        hypothesis.assume(len(name) > 0)
        hypothesis.assume(all(c in allowed_characters for c in name))
        hypothesis.assume(name[0] not in digits)

    def hypothesis_assume_variable_value_is_valid(self, value: bytes) -> None:
        invalid_control_characters = bytes(range(0, b"\n"[0])) + bytes(
            range(b"\n"[0] + 1, b" "[0])
        )
        hypothesis.assume(all(c not in invalid_control_characters for c in value))


class SystemdEnvironmentFileLoadTest(unittest.TestCase):
    # TODO(strager): Discard variables whose values are not valid UTF-8.

    def test_empty_file_has_no_variables(self) -> None:
        self.assertEqual(self.loads(b"").entries, [])

    def test_one_simple_variable(self) -> None:
        self.assertEqual(
            self.loads(b"my_variable=my_value\n").entries,
            [(b"my_variable", b"my_value")],
        )
        self.assertEqual(
            self.loads(b"my_variable=my_value").entries, [(b"my_variable", b"my_value")]
        )
        self.assertEqual(self.loads(b"MESSAGE=hello").entries, [(b"MESSAGE", b"hello")])

    def test_value_can_be_empty(self) -> None:
        self.assertEqual(self.loads(b"my_variable=\n").entries, [(b"my_variable", b"")])
        self.assertEqual(self.loads(b"my_variable=").entries, [(b"my_variable", b"")])
        self.assertEqual(
            self.loads(b"my_variable=   \t \n").entries, [(b"my_variable", b"")]
        )
        self.assertEqual(self.loads(b"my_variable=''").entries, [(b"my_variable", b"")])
        self.assertEqual(self.loads(b'my_variable=""').entries, [(b"my_variable", b"")])

    def test_multiple_variables_are_separated_by_newlines_or_carriage_returns(
        self
    ) -> None:
        self.assertEqual(
            self.loads(b"var1=value1\nvar2=value2\nvar3=value3\n").entries,
            [(b"var1", b"value1"), (b"var2", b"value2"), (b"var3", b"value3")],
        )
        self.assertEqual(
            self.loads(b"var1=value1\rvar2=value2\rvar3=value3\r").entries,
            [(b"var1", b"value1"), (b"var2", b"value2"), (b"var3", b"value3")],
        )

    def test_empty_lines_are_ignored(self) -> None:
        self.assertEqual(
            self.loads(b"\n\nvar=value\n\nvar2=value2\n\n").entries,
            [(b"var", b"value"), (b"var2", b"value2")],
        )

    def test_blank_lines_are_ignored(self) -> None:
        self.assertEqual(self.loads(b"  \n  \n\t\n").entries, [])
        self.assertEqual(self.loads(b" ").entries, [])

    def test_backslash_does_not_escape_anything_in_name(self) -> None:
        self.assertEqual(self.loads(b"a\\\nb=c").entries, [(b"b", b"c")])
        self.assertEqual(self.loads(b"\\\nname=value").entries, [(b"name", b"value")])

    def test_backslash_escapes_benign_characters_in_value(self) -> None:
        self.assertEqual(self.loads(br"var=\v\al\u\e").entries, [(b"var", b"value")])

    def test_backslash_escapes_quotes_in_value(self) -> None:
        self.assertEqual(self.loads(br"var=\'value\'").entries, [(b"var", b"'value'")])
        self.assertEqual(self.loads(br"var=\"value\"").entries, [(b"var", b'"value"')])

    def test_backslash_escapes_newlines_in_value(self) -> None:
        self.assertEqual(self.loads(b"var=a\\\nb\n").entries, [(b"var", b"ab")])

    def test_surrounding_whitespace_in_value_is_preserved_if_escaped(self) -> None:
        self.assertEqual(self.loads(br"var=\ value\ ").entries, [(b"var", b" value ")])
        self.assertEqual(
            self.loads(br"var=\  value \ ").entries, [(b"var", b"  value  ")]
        )

    def test_backslash_at_end_of_file_in_value_is_ignored(self) -> None:
        self.assertEqual(self.loads(b"var=value\\").entries, [(b"var", b"value")])

    def test_backslash_at_end_of_file_in_quoted_value_is_ignored(self) -> None:
        self.assertEqual(self.loads(b'var="value\\').entries, [(b"var", b"value")])

    def test_surrounding_whitespace_in_value_is_ignored(self) -> None:
        self.assertEqual(self.loads(b"var=  value\n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"var=value  \n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"var=\tvalue\n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"var=value\t\n").entries, [(b"var", b"value")])

    def test_surrounding_whitespace_in_quoted_value_is_ignored(self) -> None:
        self.assertEqual(self.loads(b"var= 'value' \n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b'var= "value" \n').entries, [(b"var", b"value")])

    def test_surrounding_whitespace_in_name_is_ignored(self) -> None:
        self.assertEqual(self.loads(b"  var=value\n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"var  =value\n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"\tvar=value\n").entries, [(b"var", b"value")])
        self.assertEqual(self.loads(b"var\t=value\n").entries, [(b"var", b"value")])

    def test_values_can_have_interior_whitespace(self) -> None:
        self.assertEqual(
            self.loads(b"variable=multi word value").entries,
            [(b"variable", b"multi word value")],
        )
        self.assertEqual(self.loads(b"var=a\tb").entries, [(b"var", b"a\tb")])

    def test_values_can_contain_equal_sign(self) -> None:
        self.assertEqual(
            self.loads(b"variable=value=with=equal=signs").entries,
            [(b"variable", b"value=with=equal=signs")],
        )

    def test_redundant_quotes_in_values_are_dropped(self) -> None:
        self.assertEqual(self.loads(b"name='value'").entries, [(b"name", b"value")])
        self.assertEqual(self.loads(b'name="value"').entries, [(b"name", b"value")])

        self.assertEqual(self.loads(b"name='a''b'").entries, [(b"name", b"ab")])
        self.assertEqual(self.loads(b'name="a""b"').entries, [(b"name", b"ab")])

        self.assertEqual(
            self.loads(b"name='hello'world").entries, [(b"name", b"helloworld")]
        )
        self.assertEqual(
            self.loads(b'name="hello"world').entries, [(b"name", b"helloworld")]
        )

        self.assertEqual(self.loads(b"""name="a"'b'""").entries, [(b"name", b"ab")])
        self.assertEqual(self.loads(b'''name='a'"b"''').entries, [(b"name", b"ab")])

    def test_quotes_in_values_are_included_verbatim_after_unquoted_nonwhitespace_characters(
        self
    ) -> None:
        self.assertEqual(
            self.loads(b"name=hello'world'").entries, [(b"name", b"hello'world'")]
        )
        self.assertEqual(
            self.loads(b"name=hello'world").entries, [(b"name", b"hello'world")]
        )
        self.assertEqual(
            self.loads(b"name=hello 'world'").entries, [(b"name", b"hello 'world'")]
        )
        self.assertEqual(
            self.loads(b"name='a b' c d 'e f' g h").entries,
            [(b"name", b"a bc d 'e f' g h")],
        )
        self.assertEqual(
            self.loads(b'name="a b" c d "e f" g h').entries,
            [(b"name", b'a bc d "e f" g h')],
        )

    def test_values_can_have_surrounding_whitespace_within_quotes(self) -> None:
        self.assertEqual(self.loads(b"name=' value '").entries, [(b"name", b" value ")])
        self.assertEqual(self.loads(b'name=" value "').entries, [(b"name", b" value ")])

    def test_whitespace_after_quoted_string_in_value_is_ignored(self) -> None:
        self.assertEqual(self.loads(b"name='' value").entries, [(b"name", b"value")])
        self.assertEqual(self.loads(b'name="" value').entries, [(b"name", b"value")])
        self.assertEqual(self.loads(b"name='a' value").entries, [(b"name", b"avalue")])
        self.assertEqual(self.loads(b'name="a" value').entries, [(b"name", b"avalue")])
        self.assertEqual(
            self.loads(b"name='hello' 'world'").entries, [(b"name", b"helloworld")]
        )
        self.assertEqual(
            self.loads(b"name='hello' \"world\"").entries, [(b"name", b"helloworld")]
        )

    def test_values_can_have_newlines_within_quotes(self) -> None:
        self.assertEqual(self.loads(b"name='a\nb'").entries, [(b"name", b"a\nb")])
        self.assertEqual(self.loads(b'name="a\nb"').entries, [(b"name", b"a\nb")])

    def test_backslash_escapes_benign_characters_in_quoted_value(self) -> None:
        self.assertEqual(self.loads(b"name='a \\n b'").entries, [(b"name", b"a n b")])
        self.assertEqual(self.loads(b'name="a \\n b"').entries, [(b"name", b"a n b")])

    def test_backslash_escapes_newline_in_quoted_value(self) -> None:
        self.assertEqual(self.loads(b"name='a\\\nb'").entries, [(b"name", b"ab")])
        self.assertEqual(self.loads(b'name="a\\\nb"').entries, [(b"name", b"ab")])

    def test_backslash_escapes_backslash_in_quoted_value(self) -> None:
        self.assertEqual(self.loads(b"name='a\\\\b'").entries, [(b"name", b"a\\b")])

    def test_backslash_escapes_quotes_in_quoted_value(self) -> None:
        self.assertEqual(
            self.loads(br"""name=' \' \" '""").entries, [(b"name", b" ' \" ")]
        )
        self.assertEqual(
            self.loads(br'''name=" \' \" "''').entries, [(b"name", b" ' \" ")]
        )

    def test_double_quotes_are_benign_within_single_quotes(self) -> None:
        self.assertEqual(
            self.loads(br"""name='hello "world"'""").entries,
            [(b"name", b'hello "world"')],
        )
        self.assertEqual(self.loads(br"""name='"'""").entries, [(b"name", b'"')])

    def test_single_quotes_are_benign_within_double_quotes(self) -> None:
        self.assertEqual(
            self.loads(br'''name="hello 'world'"''').entries,
            [(b"name", b"hello 'world'")],
        )
        self.assertEqual(self.loads(br'''name="'"''').entries, [(b"name", b"'")])
        self.assertEqual(
            self.loads(br"""name="I can't "'even'...""").entries,
            [(b"name", b"I can't even...")],
        )

    def test_value_with_unescaped_quote_extends_to_end_of_file(self) -> None:
        self.assertEqual(
            self.loads(b"name='value\nname2=value2\n").entries,
            [(b"name", b"value\nname2=value2\n")],
        )

    def test_values_can_contain_comment_markers(self) -> None:
        self.assertEqual(
            self.loads(b"variable=value;with#comment").entries,
            [(b"variable", b"value;with#comment")],
        )

    def test_lines_without_equal_sign_are_ignored(self) -> None:
        self.assertEqual(self.loads(b"notavariable\n").entries, [])

    def test_comment_lines_are_ignored(self) -> None:
        self.assertEqual(self.loads(b"#").entries, [])
        self.assertEqual(self.loads(b"#\\").entries, [])
        self.assertEqual(self.loads(b"#var=value\n").entries, [])
        self.assertEqual(self.loads(b";var=value\n").entries, [])
        self.assertEqual(self.loads(b"  # var=value\n").entries, [])
        self.assertEqual(self.loads(b"\t# var=value\n").entries, [])

    def test_backslash_escapes_newline_in_comment_line(self) -> None:
        self.assertEqual(
            self.loads(
                b"#\\\n" b"var1=value1\\\n" b"var2=value2\n" b"var3=value3\n"
            ).entries,
            [(b"var3", b"value3")],
        )
        self.assertEqual(
            self.loads(b"#\\\n\nvar=value\n").entries, [(b"var", b"value")]
        )
        self.assertEqual(self.loads(b"#\\\n\\\nvar=value\n").entries, [])

    def test_backslash_escapes_backslash_in_comment_line(self) -> None:
        self.assertEqual(self.loads(b"#\\\\\nvar=value").entries, [(b"var", b"value")])

    def test_name_can_contain_underscores(self) -> None:
        self.assertEqual(self.loads(b"name_=value").entries, [(b"name_", b"value")])
        self.assertEqual(self.loads(b"_name=value").entries, [(b"_name", b"value")])
        self.assertEqual(
            self.loads(b"__name__=value").entries, [(b"__name__", b"value")]
        )

    def test_name_starting_with_one_equal_sign_discards_entire_variable(self) -> None:
        self.assertEqual(self.loads(b"=name=value").entries, [])
        self.assertEqual(self.loads(b"==a=b").entries, [])
        self.assertEqual(
            self.loads(b"=name='value\nsame=variable'").entries,
            [],
            "same=variable should be parsed as part of the value for the "
            "discarded variable",
        )

    def test_name_with_symbol_discards_entire_variable(self) -> None:
        for symbol_byte in b"`~!@$%^&*()[]+-.,/?'\"\\|":
            symbol = bytes([symbol_byte])
            with self.subTest(symbol=symbol):
                self.assertEqual(
                    self.loads(b"var" + symbol + b"name=value").entries, []
                )
                self.assertEqual(
                    self.loads(
                        b"var" + symbol + b"name='hello\nsame=variable'"
                    ).entries,
                    [],
                    "same=variable should be parsed as part of the value for "
                    "the discarded variable",
                )

    def test_interior_whitespace_in_name_discards_entire_variable(self) -> None:
        self.assertEqual(self.loads(b"multi word variable=value").entries, [])
        self.assertEqual(
            self.loads(b"multi word variable='hello\nsame=variable'").entries,
            [],
            "same=variable should be parsed as part of the value for the "
            "discarded variable",
        )

    def test_comment_characters_in_name_discards_entire_variable(self) -> None:
        self.assertEqual(self.loads(b"variable;with#comment=value").entries, [])
        self.assertEqual(
            self.loads(b"variable;with#comment='hello\nsame=variable'").entries,
            [],
            "same=variable should be parsed as part of the value for the "
            "discarded variable",
        )

    def test_backslash_in_name_discards_entire_variable(self) -> None:
        for name in [br"\ name", br"\\name", br"a\\b", br"\#name", b"\\"]:
            with self.subTest(name=name):
                self.assertEqual(self.loads(name + b"=value").entries, [])
                self.assertEqual(
                    self.loads(name + b"='hello\nsame=variable'").entries,
                    [],
                    "same=variable should be parsed as part of the value for "
                    "the discarded variable",
                )

    def test_non_ascii_in_name_discards_entire_variable(self) -> None:
        name_with_valid_utf8_letter = br"hell\xc3\xb6"
        name_with_invalid_utf8 = br"hello\x80world"
        for name in [name_with_valid_utf8_letter, name_with_invalid_utf8]:
            with self.subTest(name=name):
                self.assertEqual(self.loads(name + b"=value").entries, [])
                self.assertEqual(
                    self.loads(name + b"='hello\nsame=variable'").entries,
                    [],
                    "same=variable should be parsed as part of the value for "
                    "the discarded variable",
                )

    def test_leading_digit_in_name_discards_entire_variable(self) -> None:
        self.assertEqual(self.loads(b"99BOTTLES=beer").entries, [])
        self.assertEqual(
            self.loads(b"1up='hello\nsame=variable'").entries,
            [],
            "same=variable should be parsed as part of the value for the "
            "discarded variable",
        )

    def test_carriage_return_in_value_discards_entire_variable(self) -> None:
        self.assertEqual(self.loads(b"var='hello\rworld'").entries, [])
        self.assertEqual(
            self.loads(b"var='hello\rsame=variable'").entries,
            [],
            "same=variable should be parsed as part of the value for the "
            "discarded variable",
        )

    def test_control_characters_in_value_discards_entire_variable(self) -> None:
        for control_character_byte in b"\x01\a\x1b":
            control_character = bytes([control_character_byte])
            with self.subTest(control_character=control_character):
                self.assertEqual(
                    self.loads(
                        b"var=control" + control_character + b"character\n"
                    ).entries,
                    [],
                )

    def test_null_byte_is_treated_as_end_of_file(self) -> None:
        self.assertEqual(self.loads(b"name=value\x00").entries, [(b"name", b"value")])
        self.assertEqual(
            self.loads(b"name=hello\x00world").entries, [(b"name", b"hello")]
        )

    @fast_hypothesis_test()
    @hypothesis.given(hypothesis.strategies.binary())
    def test_loading_arbitrary_file_does_not_crash_or_hang(
        self, content: bytes
    ) -> None:
        self.loads(content)

    def loads(self, content: bytes) -> SystemdEnvironmentFile:
        return SystemdEnvironmentFile.loads(content)


class EscapeDBusAddressTest(unittest.TestCase):
    def test_escaped_empty_address_is_empty(self) -> None:
        self.assertEqual(escape_dbus_address(b""), b"")

    def test_alphabet_is_not_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b"abc"), b"abc")
        self.assertEqual(escape_dbus_address(b"ABC"), b"ABC")

    def test_digits_are_not_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b"0123456789"), b"0123456789")

    def test_slashes_are_not_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b"/"), b"/")
        self.assertEqual(escape_dbus_address(b"/path/to/bus"), b"/path/to/bus")
        self.assertEqual(escape_dbus_address(b"\\"), b"\\")

    def test_dots_and_dashes_are_not_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b".-"), b".-")
        self.assertEqual(escape_dbus_address(b"file.txt"), b"file.txt")
        self.assertEqual(escape_dbus_address(b"hello-world"), b"hello-world")

    def test_special_address_characters_are_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b":"), b"%3a")
        self.assertEqual(escape_dbus_address(b";"), b"%3b")
        self.assertEqual(escape_dbus_address(b"c:\\windows\\"), b"c%3a\\windows\\")

    def test_escape_characters_are_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b"%"), b"%25")
        self.assertEqual(escape_dbus_address(b"%25"), b"%2525")

    def test_whitespace_is_escaped(self) -> None:
        self.assertEqual(escape_dbus_address(b" "), b"%20")
        self.assertEqual(escape_dbus_address(b"\n"), b"%0a")
