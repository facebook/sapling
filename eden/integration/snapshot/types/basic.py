#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib import edenclient
from eden.integration.snapshot import verify as verify_mod
from eden.integration.snapshot.snapshot import HgSnapshot, snapshot_class


@snapshot_class(
    "basic",
    "A simple directory structure with a mix of loaded, materialized, "
    "and unloaded files.",
)
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `system_hgrc_path` is never initialized.
class BasicSnapshot(HgSnapshot):
    def populate_backing_repo(self) -> None:
        repo = self.backing_repo
        repo.write_file("README.md", "project docs")
        repo.write_file(".gitignore", "ignored.txt\n")

        repo.write_file("main/loaded_dir/loaded_file.c", "loaded")
        repo.write_file("main/loaded_dir/not_loaded_file.c", "not loaded")
        repo.write_file("main/loaded_dir/not_loaded_exe.sh", "not loaded", mode=0o755)
        repo.write_file("main/loaded_dir/not_loaded_subdir/a.txt", "some contents\n")
        repo.write_file(
            "main/loaded_dir/not_loaded_subdir/b.exe", "other contents", mode=0o755
        )
        repo.write_file("main/loaded_dir/loaded_subdir/dir1/file1.txt", "text\n")
        repo.write_file("main/loaded_dir/loaded_subdir/dir2/file2.txt", "more text\n")

        repo.write_file(
            "main/materialized_subdir/script.sh", "original script contents", mode=0o755
        )
        repo.write_file("main/materialized_subdir/test.c", "original test contents")
        repo.write_file("main/materialized_subdir/unmodified.txt", "original contents")
        repo.symlink("main/materialized_subdir/modified_symlink.lnk", "original link")
        repo.write_file("main/mode_changes/normal_to_exe.txt", "will change mode")
        repo.write_file(
            "main/mode_changes/exe_to_normal.txt", "will change mode", mode=0o755
        )
        repo.write_file("main/mode_changes/normal_to_readonly.txt", "will be readonly")

        repo.write_file("never_accessed/foo/bar/baz.txt", "baz\n")
        repo.write_file("never_accessed/foo/bar/xyz.txt", "xyz\n")
        repo.write_file("never_accessed/foo/file.txt", "data\n")
        repo.symlink("never_accessed/foo/some.lnk", "link destination")
        repo.commit("Initial commit.")

    def populate_checkout(self) -> None:
        # Load the main/loaded_dir directory and some of its children.
        # Listing directories will allocate inode numbers for its children which causes
        # it to be tracked in the overlay, even if it has not been modified.
        self.list_dir("main/loaded_dir")
        self.list_dir("main/loaded_dir/loaded_subdir/dir1")
        self.list_dir("main/loaded_dir/loaded_subdir/dir2")
        self.read_file("main/loaded_dir/loaded_file.c")
        self.read_file("main/loaded_dir/loaded_subdir/dir1/file1.txt")
        self.read_file("main/loaded_dir/loaded_subdir/dir2/file2.txt")

        # Modify some files in main/materialized_subdir to force them to be materialized
        self.write_file(
            "main/materialized_subdir/script.sh", b"new script contents", 0o755
        )
        self.write_file("main/materialized_subdir/test.c", b"new test contents")
        self.symlink("main/materialized_subdir/modified_symlink.lnk", b"new link")
        self.symlink("main/materialized_subdir/new_symlink.lnk", b"new link")
        self.make_socket("main/materialized_subdir/test/socket.sock", mode=0o600)

        # Test materializing some files by changing their mode
        self.chmod("main/mode_changes/normal_to_exe.txt", 0o755)
        self.chmod("main/mode_changes/exe_to_normal.txt", 0o644)
        self.chmod("main/mode_changes/normal_to_readonly.txt", 0o400)

        # Create a new top-level directory with some new files
        self.write_file("untracked/new/normal.txt", b"new src contents")
        self.write_file("untracked/new/normal2.txt", b"extra src contents")
        self.write_file("untracked/new/readonly.txt", b"new readonly contents", 0o400)
        self.write_file("untracked/new/subdir/abc.txt", b"abc")
        self.write_file("untracked/new/subdir/xyz.txt", b"xyz")
        self.write_file("untracked/executable.exe", b"do stuff", mode=0o755)
        self.make_socket("untracked/everybody.sock", mode=0o666)
        self.make_socket("untracked/owner_only.sock", mode=0o600)

        # Create some untracked files in an existing tracked directory
        self.write_file("main/untracked.txt", b"new new untracked file")
        self.write_file("main/ignored.txt", b"new ignored file")
        self.write_file("main/untracked_dir/foo.txt", b"foobar")

    def get_expected_files(self) -> verify_mod.ExpectedFileSet:
        # Confirm that the files look like what we expect
        files = verify_mod.ExpectedFileSet()

        # TODO: These symlink permissions should ideally be 0o777
        files.add_symlink(".eden/root", bytes(self.checkout_path), 0o770)
        files.add_symlink(
            ".eden/client", bytes(self.eden_state_dir / "clients" / "checkout"), 0o770
        )
        files.add_symlink(".eden/socket", bytes(self.eden_state_dir / "socket"), 0o770)
        files.add_symlink(".eden/this-dir", bytes(self.checkout_path / ".eden"), 0o770)
        files.add_file("README.md", b"project docs", 0o644)
        files.add_file(".gitignore", b"ignored.txt\n", 0o644)
        files.add_file("main/loaded_dir/loaded_file.c", b"loaded", 0o644)
        files.add_file("main/loaded_dir/not_loaded_file.c", b"not loaded", 0o644)
        files.add_file("main/loaded_dir/not_loaded_exe.sh", b"not loaded", 0o755)
        files.add_file("main/loaded_dir/loaded_subdir/dir1/file1.txt", b"text\n", 0o644)
        files.add_file(
            "main/loaded_dir/loaded_subdir/dir2/file2.txt", b"more text\n", 0o644
        )
        files.add_file(
            "main/loaded_dir/not_loaded_subdir/a.txt", b"some contents\n", 0o644
        )
        files.add_file(
            "main/loaded_dir/not_loaded_subdir/b.exe", b"other contents", 0o755
        )
        files.add_file(
            "main/materialized_subdir/script.sh", b"new script contents", 0o755
        )
        files.add_file("main/materialized_subdir/test.c", b"new test contents", 0o644)
        files.add_file(
            "main/materialized_subdir/unmodified.txt", b"original contents", 0o644
        )
        files.add_symlink(
            "main/materialized_subdir/modified_symlink.lnk", b"new link", 0o770
        )
        files.add_symlink(
            "main/materialized_subdir/new_symlink.lnk", b"new link", 0o770
        )
        files.add_socket("main/materialized_subdir/test/socket.sock", 0o600)
        files.add_file(
            "main/mode_changes/normal_to_exe.txt", b"will change mode", 0o755
        )
        files.add_file(
            "main/mode_changes/exe_to_normal.txt", b"will change mode", 0o644
        )
        files.add_file(
            "main/mode_changes/normal_to_readonly.txt", b"will be readonly", 0o400
        )
        files.add_file("main/untracked.txt", b"new new untracked file", 0o644)
        files.add_file("main/ignored.txt", b"new ignored file", 0o644)
        files.add_file("main/untracked_dir/foo.txt", b"foobar", 0o644)
        files.add_file("never_accessed/foo/bar/baz.txt", b"baz\n", 0o644)
        files.add_file("never_accessed/foo/bar/xyz.txt", b"xyz\n", 0o644)
        files.add_file("never_accessed/foo/file.txt", b"data\n", 0o644)
        files.add_symlink("never_accessed/foo/some.lnk", b"link destination", 0o755)
        files.add_file("untracked/new/normal.txt", b"new src contents", 0o644)
        files.add_file("untracked/new/normal2.txt", b"extra src contents", 0o644)
        files.add_file("untracked/new/readonly.txt", b"new readonly contents", 0o400)
        files.add_file("untracked/new/subdir/abc.txt", b"abc", 0o644)
        files.add_file("untracked/new/subdir/xyz.txt", b"xyz", 0o644)
        files.add_file("untracked/executable.exe", b"do stuff", 0o755)
        files.add_socket("untracked/everybody.sock", 0o666)
        files.add_socket("untracked/owner_only.sock", 0o600)
        return files

    def verify_snapshot_data(
        self, verifier: verify_mod.SnapshotVerifier, eden: edenclient.EdenFS
    ) -> None:
        # Confirm that `hg status` reports the correct information
        self.verify_hg_status(verifier)

        expected_files = self.get_expected_files()
        verifier.verify_directory(self.checkout_path, expected_files)

    def verify_hg_status(self, verifier: verify_mod.SnapshotVerifier) -> None:
        expected_status = {
            "main/materialized_subdir/script.sh": "M",
            "main/materialized_subdir/test.c": "M",
            "main/materialized_subdir/modified_symlink.lnk": "M",
            "main/materialized_subdir/new_symlink.lnk": "?",
            "main/materialized_subdir/test/socket.sock": "?",
            "main/mode_changes/normal_to_exe.txt": "M",
            "main/mode_changes/exe_to_normal.txt": "M",
            # We changed the mode on main/mode_changes/normal_to_readonly.txt,
            # but the change isn't significant to mercurial.
            "untracked/new/normal.txt": "?",
            "untracked/new/normal2.txt": "?",
            "untracked/new/readonly.txt": "?",
            "untracked/new/subdir/abc.txt": "?",
            "untracked/new/subdir/xyz.txt": "?",
            "untracked/executable.exe": "?",
            "untracked/everybody.sock": "?",
            "untracked/owner_only.sock": "?",
            "main/untracked.txt": "?",
            "main/ignored.txt": "I",
            "main/untracked_dir/foo.txt": "?",
        }
        repo = self.hg_repo(self.checkout_path)
        verifier.verify_hg_status(repo, expected_status)
