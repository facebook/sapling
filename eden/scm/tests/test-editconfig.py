# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import tempfile
import unittest
from pathlib import Path
from typing import Optional

import silenttestrunner
from edenscm.rcutil import editconfig


class testeditconfig(unittest.TestCase):
    def setUp(self):
        fd, path = tempfile.mkstemp()
        os.close(fd)
        self.path = Path(path)

    def tearDown(self):
        self.path.unlink()

    def testaddconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            "val",
            """
[sec1]
name1 = val
""".lstrip(),
        )

        self.assertedit(
            "sec2",
            "name1",
            "val",
            """
[sec1]
name1 = val

[sec2]
name1 = val
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            "dont\nmessup",
            """
[sec1]
name1 = val
name2 = dont
  messup

[sec2]
name1 = val
""".lstrip(),
        )

    def testeditconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            "foo\nbar",
            """
[sec1]
name1 = foo
  bar
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name1",
            "baz",
            """
[sec1]
name1 = baz
""".lstrip(),
        )

    def testdeleteconfig(self):
        self.assertedit(
            "sec1",
            "name1",
            None,
            "",
        )

        self.assertedit(
            "sec1",
            "name1",
            "foo",
            """
[sec1]
name1 = foo
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            "bar\nbaz",
            """
[sec1]
name1 = foo
name2 = bar
  baz
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name1",
            None,
            """
[sec1]
name2 = bar
  baz
""".lstrip(),
        )

        self.assertedit(
            "sec1",
            "name2",
            None,
            """
[sec1]
""".lstrip(),
        )

    def assertedit(self, section: str, name: str, value: Optional[str], expected: str):
        editconfig(self.path, section, name, value)
        self.assertEqual(self.path.read_text(), expected)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
