#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import re
import unittest
from pathlib import Path
from typing import Optional, Union

from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .. import config as config_mod


class CheckoutInfo:
    """A helper class for creating fake Eden checkouts in tests."""

    def __init__(self, path: Path, name: str, eden_state_dir: Path) -> None:
        self.path = path
        self.name = name
        self.eden_state_dir = eden_state_dir

    @property
    def checkout_state_dir(self) -> Path:
        return self.eden_state_dir.joinpath("clients", self.name)

    @property
    def eden_socket_path(self) -> Path:
        return self.eden_state_dir.joinpath("socket")

    def joinpath(self, *args: Union[Path, str]) -> Path:
        return self.path.joinpath(*args)

    def make_dirs(self, path: Path, with_dot_eden: bool = True) -> None:
        assert not path.is_absolute()
        # Make sure the checkout root exists
        try:
            self.path.mkdir()
        except FileExistsError:
            pass
        else:
            # We always create the .eden directory in the checkout root
            self.make_eden_subdir(self.path)

        curdir = self.path
        for name in path.parts:
            curdir = curdir.joinpath(name)
            try:
                curdir.mkdir()
            except FileExistsError:
                continue
            if with_dot_eden:
                self.make_eden_subdir(curdir)

    def make_eden_subdir(self, path: Path) -> None:
        eden_dir = path.joinpath(".eden")
        eden_dir.mkdir()
        os.symlink(str(self.eden_socket_path), eden_dir.joinpath("socket"))
        os.symlink(str(self.checkout_state_dir), eden_dir.joinpath("client"))
        os.symlink(str(self.path), eden_dir.joinpath("root"))


class FindEdenTest(unittest.TestCase, TemporaryDirectoryMixin):
    def setUp(self) -> None:
        self._tmp_path = Path(self.make_temporary_directory())
        self._home_dir = self._tmp_path.joinpath("home")
        self._home_dir.mkdir()
        self._etc_eden_dir = self._tmp_path.joinpath("etc_eden")
        self._etc_eden_dir.mkdir()

        # TODO: this path is currently hard-coded in config.py, but should eventually
        # be loaded from a config file.
        self._eden_state_dir = self._tmp_path.joinpath("home", "local", ".eden")
        self._eden_state_dir.mkdir(parents=True)

    def _define_checkout(
        self,
        path: Union[Path, str],
        name: Optional[str] = None,
        eden_state_dir: Optional[Path] = None,
    ) -> CheckoutInfo:
        abs_path = self._tmp_path.joinpath(path)
        if name is None:
            name = abs_path.name
        assert not abs_path.exists()

        if eden_state_dir is None:
            eden_state_dir = self._eden_state_dir

        # We intentionally use our own custom config logic here
        # rather than the production code that we are trying to test.
        config_path = eden_state_dir.joinpath("config.json")
        try:
            with config_path.open("r") as f:
                config_data = json.load(f)
        except OSError:
            config_data = {}

        config_data[str(abs_path)] = name
        with config_path.open("w") as f:
            json.dump(config_data, f, indent=2, sort_keys=True)

        return CheckoutInfo(abs_path, name, eden_state_dir)

    def test_not_a_checkout(self) -> None:
        (instance, checkout, rel_path) = config_mod.find_eden(
            "/", str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        self.assertIsNone(checkout)
        self.assertIsNone(rel_path)

        (instance, checkout, rel_path) = config_mod.find_eden(
            self._tmp_path, str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        self.assertIsNone(checkout)
        self.assertIsNone(rel_path)

    def test_not_mounted(self) -> None:
        checkout_info = self._define_checkout("checkout")

        sub_path = Path("foo/bar/asdf")
        (instance, checkout, rel_path) = config_mod.find_eden(
            checkout_info.joinpath(sub_path),
            str(self._etc_eden_dir),
            str(self._home_dir),
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, sub_path)

        (instance, checkout, rel_path) = config_mod.find_eden(
            checkout_info.path, str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, Path("."))

        # Using the temporary directory root should still return no checkout even with
        # the one above defined.
        (instance, checkout, rel_path) = config_mod.find_eden(
            self._tmp_path, str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        self.assertIsNone(checkout)
        self.assertIsNone(rel_path)

    def test_mounted(self) -> None:
        checkout_info = self._define_checkout("checkout")

        # Test looking up a directory in the checkout
        subdir = Path("foo/bar/baz")
        checkout_info.make_dirs(subdir)
        (instance, checkout, rel_path) = config_mod.find_eden(
            checkout_info.joinpath(subdir), str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, subdir)

        # Test looking up using a file in the checkout
        subfile = subdir.joinpath("main.c")
        with checkout_info.joinpath(subfile).open("w") as f:
            f.write("\n")

        (instance, checkout, rel_path) = config_mod.find_eden(
            checkout_info.joinpath(subfile),
            str(self._etc_eden_dir),
            str(self._home_dir),
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, subfile)

        # Test looking up a directory in the checkout that does not contain a .eden
        # subdirectory.  This can occur if the directory is inside a bind-mount in the
        # checkout.
        bind_mount_subdir = subdir.joinpath("buck-out", "dev", "gen")
        abs_bind_mount_subdir = checkout_info.joinpath(bind_mount_subdir)
        abs_bind_mount_subdir.mkdir(parents=True)
        (instance, checkout, rel_path) = config_mod.find_eden(
            abs_bind_mount_subdir, str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), self._eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, bind_mount_subdir)

    def test_mounted_alt_instance(self) -> None:
        """Test a mount point with .eden symlinks pointing to an alternate edenfs
        instance, and confirm that the returned EdenInstance is correct.
        """
        alt_eden_state_dir = self._tmp_path.joinpath("alt_eden")
        alt_eden_state_dir.mkdir()

        checkout_info = self._define_checkout(
            "checkout", eden_state_dir=alt_eden_state_dir
        )

        # Test looking up a directory in the checkout
        subdir = Path("foo/bar/baz")
        checkout_info.make_dirs(subdir)
        (instance, checkout, rel_path) = config_mod.find_eden(
            checkout_info.joinpath(subdir), str(self._etc_eden_dir), str(self._home_dir)
        )
        self.assertEqual(Path(instance._config_dir), alt_eden_state_dir)
        assert checkout is not None
        self.assertEqual(checkout.path, checkout_info.path)
        self.assertEqual(rel_path, subdir)

        # Trying to do the lookup and requesting the default state directory should fail
        expected_msg = (
            f"the specified directory is managed by the edenfs instance at "
            f"{alt_eden_state_dir}, which is different from the explicitly requested "
            f"instance at {self._eden_state_dir}"
        )
        with self.assertRaisesRegex(Exception, re.escape(expected_msg)):
            (instance, checkout, rel_path) = config_mod.find_eden(
                checkout_info.joinpath(subdir),
                str(self._etc_eden_dir),
                str(self._home_dir),
                state_dir=str(self._eden_state_dir),
            )
