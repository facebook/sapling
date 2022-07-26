# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import os
import time
from typing import Dict, List, Optional

from eden.integration.hg.lib.hg_extension_test_base import EdenHgTestCase
from eden.integration.lib import hgrepo

from .lib import testcase

POST_CHECKOUT_UNLOADING_DELAY_S = 2


@testcase.eden_test
# pyre-ignore[13]: T62487924
class StaleInodeTestHgNFS(EdenHgTestCase):
    commit0: str
    commit1: str

    # normally we use the NFSTestMixin to provide this, but multiple inheritance
    # gets tricky.
    def use_nfs(self) -> bool:
        return True

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "bonjour\n")
        self.commit0 = repo.commit("Commit 0.")

        repo.write_file("hola", "hello\n")
        self.commit1 = repo.commit("Commit 1.")

    # turn on inode unloading
    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        parent_config = super().edenfs_extra_config()
        if parent_config is None:
            parent_config = {}
        if "nfs" not in parent_config:
            parent_config["nfs"] = []

        parent_config["nfs"].append(
            f'post-checkout-inode-unloading-delay = "{POST_CHECKOUT_UNLOADING_DELAY_S}s"'
        )
        parent_config["nfs"].append("unload-unlinked-inodes = true")

        return parent_config

    # asserts the behavior that we do not unload inodes after rms. This behavior
    # is garunteed by the NFS client as removes are just renames to a hidden file
    # until all file handles are closed.
    def test_remove_file_no_update(self) -> None:
        bonjour = os.path.join(self.mount, "bonjour")
        with open(bonjour, "wb") as fd:
            fd.write(b"hola\n")

        with open(bonjour, "r") as fd:

            os.remove(bonjour)

            # this should not error or crash, eden does not remove the inode after
            # an rm
            fd.read()

    # asserts that even rmed files are not unloaded after update. They will be
    # unlinked after all file handles referencing them are gone, and then
    # after the next checkout they will be unloaded.
    def test_remove_file_update(self) -> None:

        nihao = os.path.join(self.mount, "nihao")
        with open(nihao, "wb") as fd:
            fd.write(b"hey\n")

        with open(nihao, "r") as fd:

            os.remove(nihao)

            self.repo.update(self.commit0, clean=True)

            # now n seconds later all the inodes are cleaned up.
            time.sleep(POST_CHECKOUT_UNLOADING_DELAY_S * 2)

            # this should not error as well because removing a file on nfs
            # actually moves the file to a hidden location until the
            # open handles to the file are all closed.
            fd.read()

    # tests files removed during a checkout are unloaded after that checkout.
    def test_update(self) -> None:
        hola = self.get_path("hola")
        with open(hola, "r") as fd:
            self.repo.update(self.commit0)
            time.sleep(POST_CHECKOUT_UNLOADING_DELAY_S * 2)
            # this should error as the file has been unlinked during checkout.
            # and the inode should be unloaded by the periodic unloading
            self.assertRaises(OSError, fd.read)

    # tests the counter is properly updated when unloading is run.
    def test_unlinked_unload_counter(self) -> None:
        counter_name = "inodemap.main.unloaded_unlinked_inodes"
        old_unloaded_count = self.get_counters()[counter_name]

        # load an inode that will be removed by checkout
        hola = self.get_path("hola")
        with open(hola, "r") as fd:
            fd.read()

        self.repo.update(self.commit0)
        # checkout triggers an unlinked inode unload
        # POST_CHECKOUT_UNLOADING_DELAY_S later
        time.sleep(POST_CHECKOUT_UNLOADING_DELAY_S * 2)

        new_unloaded_counter = self.get_counters()[counter_name]

        # at least hola should have been unloaded.
        self.assertLessEqual(old_unloaded_count + 1, new_unloaded_counter)
