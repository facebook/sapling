# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import io
import unittest

import eden.dirstate


class DirstateReadTest(unittest.TestCase):
    def test_read_sample_dirstate_1(self):
        raw_dirstate = (
            b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\xff\xf7o\x16M\xb5X^%\x92\xe7\xe4e\x8c\xa6"
            b"\xba\xfe\x1a_~\x83\xf3M\xc3\x97\xbd\xb7D.W\xa9\x8f\x9b"
        )
        with io.BytesIO(raw_dirstate) as dirstate_file:
            parents, tuples_dict, copymap = eden.dirstate.read(
                dirstate_file, "raw_dirstate"
            )
            self.assertEqual(
                parents,
                (b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr", b"\x00" * 20),
            )
            self.assertEqual(tuples_dict, {})
            self.assertEqual(copymap, {})

    def test_read_sample_dirstate_2(self):
        raw_dirstate = (
            b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\x01a\x00\x00\x00\x00\xff\x00$fbcode/eden/"
            b"py/test/dirstate_test.py\x01a\x00\x00\x00\x00\xff\x00\x1bfbcode/"
            b"eden/py/test/TARGETS\xffh\x0f,\x18\xaa\xbb\x0b\x02x\\.\xf6\x19S"
            b"\xe8\xc2#\x8b\xde\xd4\xa6s\xcf\xa1\xb9\xaekJ\x85HCW"
        )
        with io.BytesIO(raw_dirstate) as dirstate_file:
            parents, tuples_dict, copymap = eden.dirstate.read(
                dirstate_file, "raw_dirstate"
            )
            self.assertEqual(
                parents,
                (b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr", b"\x00" * 20),
            )
            self.assertEqual(
                tuples_dict,
                {
                    "fbcode/eden/py/test/dirstate_test.py": ("a", 0, -1),
                    "fbcode/eden/py/test/TARGETS": ("a", 0, -1),
                },
            )
            self.assertEqual(copymap, {})

    def test_read_sample_dirstate_3(self):
        raw_dirstate = (
            b"\xa8umh0M\xfbGO\xc5\xe2\xc4p\xe0\xd2I<\x1a\x9d\x01\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\x01a\x00\x00\x00\x00\xff\x00\x1cfbcode/eden/"
            b"py/test/TARGETS4\x02\x00\x1cfbcode/eden/py/test/TARGETS4\x00\x1b"
            b"fbcode/eden/py/test/TARGETS\xffg\x19\xdf0M\x95F\x81Y\x0b\xf3\xa3"
            b"\xbb\x82\xaf\xb5D;\x02Q*7\xc8\xcd\xe3\x1e\x98\xf6\xe8\x97\x13\xa0"
        )
        with io.BytesIO(raw_dirstate) as dirstate_file:
            parents, tuples_dict, copymap = eden.dirstate.read(
                dirstate_file, "raw_dirstate"
            )
            self.assertEqual(
                parents,
                (b"\xa8umh0M\xfbGO\xc5\xe2\xc4p\xe0\xd2I<\x1a\x9d\x01", b"\x00" * 20),
            )
            self.assertEqual(
                tuples_dict, {"fbcode/eden/py/test/TARGETS4": ("a", 0, -1)}
            )
            self.assertEqual(
                copymap, {"fbcode/eden/py/test/TARGETS4": "fbcode/eden/py/test/TARGETS"}
            )


class DirstateWriteTest(unittest.TestCase):
    def test_write_sample_dirstate_1(self):
        expected_raw_dirstate = (
            b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\xff\xf7o\x16M\xb5X^%\x92\xe7\xe4e\x8c\xa6"
            b"\xba\xfe\x1a_~\x83\xf3M\xc3\x97\xbd\xb7D.W\xa9\x8f\x9b"
        )
        parents = (b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr", b"\x00" * 20)
        tuples_dict = {}
        copymap = {}
        with io.BytesIO() as dirstate_file:
            eden.dirstate.write(dirstate_file, parents, tuples_dict, copymap)
            self.assertEqual(dirstate_file.getvalue(), expected_raw_dirstate)

    def test_write_sample_dirstate_2(self):
        expected_raw_dirstate = (
            b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\x01a\x00\x00\x00\x00\xff\x00$fbcode/eden/"
            b"py/test/dirstate_test.py\x01a\x00\x00\x00\x00\xff\x00\x1bfbcode/"
            b"eden/py/test/TARGETS\xffh\x0f,\x18\xaa\xbb\x0b\x02x\\.\xf6\x19S"
            b"\xe8\xc2#\x8b\xde\xd4\xa6s\xcf\xa1\xb9\xaekJ\x85HCW"
        )
        parents = (b"P\x03\xc2x?z\xf1\xec\xc9\x99+\xc0\xdb\xb6n[}\x92nr", b"\x00" * 20)
        tuples_dict = {
            b"fbcode/eden/py/test/dirstate_test.py": ("a", 0, -1),
            b"fbcode/eden/py/test/TARGETS": ("a", 0, -1),
        }
        copymap = {}
        with io.BytesIO() as dirstate_file:
            eden.dirstate.write(dirstate_file, parents, tuples_dict, copymap)
            self.assertEqual(dirstate_file.getvalue(), expected_raw_dirstate)

    def test_write_sample_dirstate_3(self):
        expected_raw_dirstate = (
            b"\xa8umh0M\xfbGO\xc5\xe2\xc4p\xe0\xd2I<\x1a\x9d\x01\x00\x00\x00"
            b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
            b"\x00\x00\x00\x00\x01\x01a\x00\x00\x00\x00\xff\x00\x1cfbcode/eden/"
            b"py/test/TARGETS4\x02\x00\x1cfbcode/eden/py/test/TARGETS4\x00\x1b"
            b"fbcode/eden/py/test/TARGETS\xffg\x19\xdf0M\x95F\x81Y\x0b\xf3\xa3"
            b"\xbb\x82\xaf\xb5D;\x02Q*7\xc8\xcd\xe3\x1e\x98\xf6\xe8\x97\x13\xa0"
        )
        parents = (b"\xa8umh0M\xfbGO\xc5\xe2\xc4p\xe0\xd2I<\x1a\x9d\x01", b"\x00" * 20)
        tuples_dict = {b"fbcode/eden/py/test/TARGETS4": ("a", 0, -1)}
        copymap = {b"fbcode/eden/py/test/TARGETS4": b"fbcode/eden/py/test/TARGETS"}
        with io.BytesIO() as dirstate_file:
            eden.dirstate.write(dirstate_file, parents, tuples_dict, copymap)
            self.assertEqual(dirstate_file.getvalue(), expected_raw_dirstate)
