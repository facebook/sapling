# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from sapling import filelint, ui as uimod


class FileLintTest(unittest.TestCase):
    def test_partition_reuses_repeated_path_version(self):
        first = b"1" * 20
        second = b"2" * 20

        filetrees, targets = filelint._partitioncandidates(
            [
                (0, "x.py", first, ""),
                (1, "x.py", second, ""),
                (2, "x.py", first, "x"),
            ],
            casesensitive=True,
        )

        self.assertEqual([[("x.py", first, "")], [("x.py", second, "")]], filetrees)
        self.assertEqual({(0, "x.py"): [0, 2], (1, "x.py"): [1]}, targets)

    def test_partition_splits_case_collisions_on_case_insensitive_fs(self):
        node = b"1" * 20
        candidates = [(0, "a.py", node, ""), (0, "A.py", node, "")]

        filetrees, _targets = filelint._partitioncandidates(
            candidates, casesensitive=True
        )
        self.assertEqual([[("a.py", node, ""), ("A.py", node, "")]], filetrees)

        # Case-colliding paths would alias one staged file, so they must not
        # share a tree.
        filetrees, _targets = filelint._partitioncandidates(
            candidates, casesensitive=False
        )
        self.assertEqual([[("a.py", node, "")], [("A.py", node, "")]], filetrees)

    def test_linters_load_commands_and_additive_config_files(self):
        ui = uimod.ui.load()
        ui.setconfig("filelint", "linter.arc f.command", "arc f @-")
        ui.setconfig("filelint", "linter.arc f.mode", "staging-tree")
        ui.setconfig("filelint", "linter.arc f.fix", "true")
        ui.setconfig(
            "filelint",
            "linter.arc f.config-file.prettier",
            ".prettierrc, prettier.config.js",
        )
        ui.setconfig("filelint", "linter.arc f.config-file.rust", "rustfmt.toml")
        # Validate-only linters are not supported yet.
        ui.setconfig("filelint", "linter.check.command", "check @-")
        ui.setconfig("filelint", "linter.check.mode", "staging-tree")

        linters = filelint._linters(ui)

        self.assertEqual(["arc f"], [linter.name for linter in linters])
        self.assertEqual(["arc", "f", "@-"], linters[0].command)
        self.assertEqual(
            {".prettierrc", "prettier.config.js", "rustfmt.toml"},
            filelint._configfilenames(linters),
        )
