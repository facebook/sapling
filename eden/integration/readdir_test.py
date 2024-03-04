#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import hashlib
import os
import re
import socket
import stat
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Pattern, Tuple, Union

from facebook.eden.ttypes import (
    Blake3OrError,
    DirListAttributeDataOrError,
    EdenError,
    EdenErrorType,
    FileAttributeData,
    FileAttributeDataOrError,
    FileAttributeDataOrErrorV2,
    FileAttributeDataV2,
    FileAttributes,
    GetAttributesFromFilesParams,
    GetAttributesFromFilesResult,
    GetAttributesFromFilesResultV2,
    GetConfigParams,
    ObjectIdOrError,
    ReaddirParams,
    ReaddirResult,
    Sha1OrError,
    SizeOrError,
    SourceControlType,
    SourceControlTypeOrError,
    SyncBehavior,
)

from .lib import testcase
from .lib.find_executables import FindExe

EdenThriftResult = Union[
    FileAttributeDataOrError,
    FileAttributeDataOrErrorV2,
]

# Change this if more attributes are added
ALL_ATTRIBUTES = (
    FileAttributes.FILE_SIZE
    | FileAttributes.SHA1_HASH
    | FileAttributes.SOURCE_CONTROL_TYPE
    | FileAttributes.OBJECT_ID
    | FileAttributes.BLAKE3_HASH
)


@testcase.eden_repo_test
# pyre-fixme[13]: Attribute `commit1` is never initialized.
# pyre-fixme[13]: Attribute `commit2` is never initialized.
# pyre-fixme[13]: Attribute `commit3` is never initialized.
class ReaddirTest(testcase.EdenRepoTest):
    commit1: str
    commit2: str
    commit3: str

    adir_file_id: bytes
    bdir_file_id: bytes
    hello_id: bytes
    slink_id: bytes
    adir_id: bytes
    cdir_subdir_id: bytes

    def setup_eden_test(self) -> None:
        self.enable_windows_symlinks = True
        super().setup_eden_test()

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("hash", []).append(
            'blake3-key = "20220728-2357111317192329313741#"'
        )
        return result

    def blake3_hash(self, file_path: str) -> bytes:
        key: Optional[str] = None
        with self.get_thrift_client_legacy() as client:
            config = client.getConfig(GetConfigParams())
            maybe_key = config.values.get("hash:blake3-key")
            key = (
                maybe_key.parsedValue
                if maybe_key is not None and maybe_key.parsedValue != ""
                else None
            )

            print(f"Resolved key: {maybe_key}, actual key: {key}")

        cmd = [FindExe.BLAKE3_SUM, "--file", file_path]
        if key is not None:
            cmd.extend(["--key", key])

        p = subprocess.run(
            cmd, stdout=subprocess.PIPE, stdin=subprocess.PIPE, encoding="ascii"
        )
        assert p.returncode == 0, "0 exit code is expected for blake3_sum"
        return bytes.fromhex(p.stdout)

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("test_fetch1", "testing fetch\n")
        self.repo.write_file("test_fetch2", "testing fetch\n")
        self.repo.write_file("README", "docs\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/file", "bar!\n")
        self.repo.symlink("slink", "hello")
        self.commit1 = self.repo.commit("Initial commit.")

        self.repo.write_file("bdir/file", "bar?\n")
        self.repo.write_file("cdir/subdir/new.txt", "and improved")
        self.repo.remove_file("README")
        self.commit2 = self.repo.commit("Commit 2.")

        # revert the change made to bdir/file
        self.repo.write_file("bdir/file", "bar!\n")
        self.commit3 = self.repo.commit("Commit 3.")

        self.adir_file_id = {
            "hg": b"41825fd37af5796284289a1e0770ccd3d27d4832:adir/file",
            "git": b"929efb30534598535198700b994ee438d441d1af",
        }[self.repo_type]

        self.bdir_file_id = {
            "hg": b"e5336dae10d1e7590fc25db9d417d089295875e0:bdir/file",
            "git": b"e50a49f9558d09d4d3bfc108363bb24c127ed263",
        }[self.repo_type]

        self.hello_id = {
            "hg": b"edd9bdab9ab7a84b21a7a19fffe7a29709ac3b47:hello",
            "git": b"5c1b14949828006ed75a3e8858957f86a2f7e2eb",
        }[self.repo_type]

        self.slink_id = {
            "hg": b"7fac34a232926c628f2d890d3eed95be7ab57f34:slink",
            "git": b"b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0",
        }[self.repo_type]

        self.adir_id = {
            "hg": b"6ae9e90c9c90aa85adbdab16808203e64a5163cb:adir",
            "git": b"aa0e79d49fe12527662d2d73ea839691eb472c9a",
        }[self.repo_type]

        self.cdir_subdir_id = {
            "hg": b"cd765cb479197becdca10a4baa87e983244bf24f:cdir/subdir",
            "git": b"f5497927ddcc19b41c4ca57e01ff99339f93db13",
        }[self.repo_type]

    def assert_eden_error(
        self, result: EdenThriftResult, error_message: Union[str, Pattern]
    ) -> None:
        error = result.get_error()
        self.assertIsNotNone(error)
        if isinstance(error_message, str):
            self.assertEqual(error_message, error.message)
        else:
            self.assertRegex(error.message, error_message)

    def get_attributes(
        self, files: List[bytes], req_attr: int
    ) -> GetAttributesFromFilesResult:
        with self.get_thrift_client_legacy() as client:
            thrift_params = GetAttributesFromFilesParams(
                self.mount_path_bytes,
                files,
                req_attr,
            )
            return client.getAttributesFromFiles(thrift_params)

    def get_attributes_v2(
        self, files: List[bytes], req_attr: int
    ) -> GetAttributesFromFilesResultV2:
        with self.get_thrift_client_legacy() as client:
            thrift_params = GetAttributesFromFilesParams(
                self.mount_path_bytes,
                files,
                req_attr,
            )
            return client.getAttributesFromFilesV2(thrift_params)

    def get_all_attributes(self, files: List[bytes]) -> GetAttributesFromFilesResult:
        return self.get_attributes(files, ALL_ATTRIBUTES)

    def get_all_attributes_v2(
        self, files: List[bytes]
    ) -> GetAttributesFromFilesResultV2:
        return self.get_attributes_v2(files, ALL_ATTRIBUTES)

    def wrap_expected_attributes(
        self,
        raw_attributes: Tuple[
            Optional[bytes],
            Optional[int],
            Optional[SourceControlType],
            Optional[bytes],
            Optional[bytes],
        ],
    ) -> Tuple[FileAttributeDataOrError, FileAttributeDataOrErrorV2]:
        (
            raw_sha1,
            raw_size,
            raw_type,
            raw_object_id,
            raw_blake3,
        ) = raw_attributes
        data = FileAttributeData()
        data_v2 = FileAttributeDataV2()

        if raw_sha1 is not None:
            data.sha1 = raw_sha1
            data_v2.sha1 = Sha1OrError(raw_sha1)

        if raw_blake3 is not None:
            data_v2.blake3 = Blake3OrError(raw_blake3)

        if raw_size is not None:
            data.fileSize = raw_size
            data_v2.size = SizeOrError(raw_size)

        if raw_type is not None:
            data.type = raw_type
            data_v2.sourceControlType = SourceControlTypeOrError(raw_type)

        if raw_object_id is not None:
            data_v2.objectId = ObjectIdOrError(raw_object_id)

        return (
            FileAttributeDataOrError(data),
            FileAttributeDataOrErrorV2(data_v2),
        )

    def assert_attributes_result(
        self,
        expected_result,
        expected_result_v2,
        paths,
        attributes: int = ALL_ATTRIBUTES,
    ) -> None:
        print("expected: \n{}", expected_result)
        actual_result = self.get_attributes(paths, attributes)
        print("actual: \n{}", actual_result)
        self.assertEqual(len(paths), len(actual_result.res))
        self.assertEqual(
            expected_result,
            actual_result,
        )

        print(f"expected v2: \n{expected_result_v2}")
        actual_result_v2 = self.get_attributes_v2(paths, attributes)
        print(f"actual v2: \n{actual_result_v2}")
        self.assertEqual(len(paths), len(actual_result_v2.res))
        self.assertEqual(
            expected_result_v2,
            actual_result_v2,
        )

    def test_get_attributes(self) -> None:
        # expected results for file named "hello"
        (
            expected_hello_result,
            expected_hello_result_v2,
        ) = self.wrap_expected_attributes(
            self.get_expected_file_attributes("hello", self.hello_id)
        )

        # expected results for file "adir/file"
        (expected_adir_result, expected_adir_result_v2) = self.wrap_expected_attributes(
            self.get_expected_file_attributes("adir/file", self.adir_file_id)
        )

        # list of expected_results
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
                expected_adir_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
                expected_adir_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result, expected_result_v2, [b"hello", b"adir/file"]
        )

    def test_get_size_only(self) -> None:
        # expected size result for file
        expected_hello_size = self.get_expected_file_attributes("hello", None)[1]
        (
            expected_hello_result,
            expected_hello_result_v2,
        ) = self.wrap_expected_attributes((None, expected_hello_size, None, None, None))

        # create result object for "hello"
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result, expected_result_v2, [b"hello"], FileAttributes.FILE_SIZE
        )

    def test_get_type_only(self) -> None:
        # expected size result for file
        expected_hello_type = self.get_expected_file_attributes("hello", None)[2]
        (
            expected_hello_result,
            expected_hello_result_v2,
        ) = self.wrap_expected_attributes((None, None, expected_hello_type, None, None))

        # create result object for "hello"
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result,
            expected_result_v2,
            [b"hello"],
            FileAttributes.SOURCE_CONTROL_TYPE,
        )

    def test_get_attributes_throws_for_non_existent_file(self) -> None:
        results = self.get_all_attributes([b"i_do_not_exist"])
        self.assertEqual(1, len(results.res))
        self.assert_attribute_error(
            results, "i_do_not_exist: No such file or directory", 0
        )

        results_v2 = self.get_all_attributes_v2([b"i_do_not_exist"])
        self.assertEqual(1, len(results_v2.res))
        self.assert_attribute_error(
            results_v2, "i_do_not_exist: No such file or directory", 0
        )

    def test_get_sha1_only(self) -> None:
        # expected sha1 result for file
        expected_hello_sha1 = self.get_expected_file_attributes("hello", None)[0]
        (
            expected_hello_result,
            expected_hello_result_v2,
        ) = self.wrap_expected_attributes((expected_hello_sha1, None, None, None, None))

        # create result object for "hello"
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result,
            expected_result_v2,
            [b"hello"],
            FileAttributes.SHA1_HASH,
        )

    def test_get_blake3_only(self) -> None:
        # expected blake3 result for file
        expected_hello_blake3 = self.get_expected_file_attributes("hello", None)[-1]
        (
            expected_hello_result,
            expected_hello_result_v2,
        ) = self.wrap_expected_attributes(
            (None, None, None, None, expected_hello_blake3)
        )

        # create result object for "hello"
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result,
            expected_result_v2,
            [b"hello"],
            FileAttributes.BLAKE3_HASH,
        )

    def test_get_attributes_throws_for_path_with_dot_components(self) -> None:
        results = self.get_all_attributes([b"./hello"])
        self.assertEqual(1, len(results.res))
        self.assert_attribute_error(
            results,
            re.compile(r"PathComponent must not be \."),
            0,
        )

        results_v2 = self.get_all_attributes_v2([b"./hello"])
        self.assertEqual(1, len(results_v2.res))
        self.assert_attribute_error(
            results_v2,
            re.compile(r"PathComponent must not be \."),
            0,
        )

    def test_get_attributes_throws_for_empty_string(self) -> None:
        results = self.get_all_attributes([b""])
        self.assertEqual(1, len(results.res))
        self.assert_attribute_error(results, "path cannot be the empty string", 0)

        results_v2 = self.get_all_attributes_v2([b""])
        self.assertEqual(1, len(results_v2.res))
        self.assert_attribute_error(results_v2, "path cannot be the empty string", 0)

    def test_get_attributes_directory(self) -> None:
        results = self.get_all_attributes([b"adir"])
        self.assertEqual(1, len(results.res))
        self.assert_attribute_error(results, "adir: Is a directory", 0)

        expected_adir_result_v2 = FileAttributeDataOrErrorV2(
            FileAttributeDataV2(
                Sha1OrError(
                    error=EdenError(
                        message="adir: Is a directory",
                        errorCode=21,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
                SizeOrError(
                    error=EdenError(
                        message="adir: Is a directory",
                        errorCode=21,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
                SourceControlTypeOrError(SourceControlType.TREE),
                ObjectIdOrError(self.adir_id),
                blake3=Blake3OrError(
                    error=EdenError(
                        message="adir: Is a directory",
                        errorCode=21,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
            )
        )

        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_adir_result_v2,
            ]
        )
        print(f"expected v2: \n{expected_result_v2}")
        results_v2 = self.get_all_attributes_v2([b"adir"])
        print(f"actual v2: \n{results_v2}")
        self.assertEqual(1, len(results_v2.res))
        self.assertEqual(expected_result_v2, results_v2)

    def test_get_attributes_socket(self) -> None:
        sockpath = self.get_path("adir/asock")
        # UDS are not supported in python on Win until 3.9:
        # https://bugs.python.org/issue33408
        with socket.socket(socket.AF_UNIX) as sock:
            sock.bind(sockpath)

            results = self.get_all_attributes([b"adir/asock"])
            self.assertEqual(1, len(results.res))
            self.assert_attribute_error(
                results,
                "adir/asock: file is a non-source-control type: 12: Invalid argument",
                0,
            )

            expected_adir_result_v2 = FileAttributeDataOrErrorV2(
                FileAttributeDataV2(
                    Sha1OrError(
                        error=EdenError(
                            message="adir/asock: file is a non-source-control type: 12: Invalid argument",
                            errorCode=22,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                    SizeOrError(
                        error=EdenError(
                            message="adir/asock: file is a non-source-control type: 12: Invalid argument",
                            errorCode=22,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                    SourceControlTypeOrError(SourceControlType.UNKNOWN),
                    ObjectIdOrError(None),
                    blake3=Blake3OrError(
                        error=EdenError(
                            message="adir/asock: file is a non-source-control type: 12: Invalid argument",
                            errorCode=22,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                )
            )

            expected_result_v2 = GetAttributesFromFilesResultV2(
                [
                    expected_adir_result_v2,
                ]
            )
            print(f"expected v2: \n{expected_result_v2}")
            results_v2 = self.get_all_attributes_v2([b"adir/asock"])
            print(f"actual v2: \n{results_v2}")
            self.assertEqual(1, len(results_v2.res))
            self.assertEqual(expected_result_v2, results_v2)

    def test_get_attributes_symlink(self) -> None:
        results = self.get_all_attributes([b"slink"])
        self.assertEqual(1, len(results.res))
        self.assert_attribute_error(
            results, "slink: file is a symlink: Invalid argument", 0
        )
        expected_slink_result_v2 = FileAttributeDataOrErrorV2(
            FileAttributeDataV2(
                Sha1OrError(
                    error=EdenError(
                        message="slink: file is a symlink: Invalid argument",
                        errorCode=22,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
                SizeOrError(
                    error=EdenError(
                        message="slink: file is a symlink: Invalid argument",
                        errorCode=22,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
                SourceControlTypeOrError(SourceControlType.SYMLINK),
                ObjectIdOrError(self.slink_id),
                blake3=Blake3OrError(
                    error=EdenError(
                        message="slink: file is a symlink: Invalid argument",
                        errorCode=22,
                        errorType=EdenErrorType.POSIX_ERROR,
                    )
                ),
            )
        )

        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_slink_result_v2,
            ]
        )
        print(f"expected v2: \n{expected_result_v2}")
        results_v2 = self.get_all_attributes_v2([b"slink"])
        print(f"actual v2: \n{results_v2}")
        self.assertEqual(1, len(results_v2.res))
        self.assertEqual(expected_result_v2, results_v2)

    def test_get_attributes_no_files(self) -> None:
        results = self.get_all_attributes([])
        self.assertEqual(0, len(results.res))

        results = self.get_all_attributes_v2([])
        self.assertEqual(0, len(results.res))

    def test_get_no_attributes(self) -> None:
        expected_hello_result = FileAttributeDataOrError(FileAttributeData())
        expected_hello_result_v2 = FileAttributeDataOrErrorV2(FileAttributeDataV2())

        # create result object for "hello"
        expected_result = GetAttributesFromFilesResult(
            [
                expected_hello_result,
            ]
        )
        expected_result_v2 = GetAttributesFromFilesResultV2(
            [
                expected_hello_result_v2,
            ]
        )

        self.assert_attributes_result(
            expected_result,
            expected_result_v2,
            [b"hello"],
            0,
        )

    def assert_attribute_error(
        self,
        attribute_result: Union[
            GetAttributesFromFilesResult, GetAttributesFromFilesResultV2
        ],
        error_message: Union[str, Pattern],
        map_entry: int,
    ) -> None:
        self.assertIsNotNone(
            attribute_result, msg="Must pass a GetAttributesFromFilesResult"
        )
        attr_or_err = attribute_result.res[map_entry]
        expected_error = (
            FileAttributeDataOrError.ERROR
            if isinstance(attribute_result, GetAttributesFromFilesResult)
            else FileAttributeDataOrErrorV2.ERROR
        )
        self.assertEqual(
            expected_error,
            attr_or_err.getType(),
            msg="GetAttributesFromFilesResult must be an error",
        )
        self.assert_eden_error(attr_or_err, error_message)

    def get_expected_file_attributes(
        self,
        path: str,
        object_id: Optional[bytes],
    ) -> Tuple[bytes, int, SourceControlType, Optional[bytes], bytes]:
        """Get attributes for the file with the specified path inside
        the eden repository. For now, just sha1 and file size.
        """
        fullpath = self.get_path(path)
        file_stat = os.stat(fullpath, follow_symlinks=False)
        file_type = SourceControlType.REGULAR_FILE
        if stat.S_ISDIR(file_stat.st_mode):
            return (
                (0).to_bytes(20, byteorder="big"),
                0,
                SourceControlType.TREE,
                object_id,
                (0).to_bytes(32, byteorder="big"),
            )
        if stat.S_ISLNK(file_stat.st_mode):
            return (
                (0).to_bytes(20, byteorder="big"),
                0,
                SourceControlType.SYMLINK,
                object_id,
                (0).to_bytes(32, byteorder="big"),
            )
        if not stat.S_ISREG(file_stat.st_mode):
            return (
                (0).to_bytes(20, byteorder="big"),
                0,
                SourceControlType.UNKNOWN,
                object_id,
                (0).to_bytes(32, byteorder="big"),
            )
        if stat.S_IXUSR & file_stat.st_mode:
            file_type = SourceControlType.EXECUTABLE_FILE
        file_size = file_stat.st_size
        ifile = open(fullpath, "rb")
        file_contents = ifile.read()
        sha1_hash = hashlib.sha1(file_contents).digest()
        ifile.close()

        return (
            sha1_hash,
            file_size,
            file_type,
            object_id,
            self.blake3_hash(fullpath),
        )

    def get_counter(self, name: str) -> float:
        return self.get_counters()[name]

    def constructReaddirResult(
        self,
        expected_attributes: Tuple[
            bytes, int, SourceControlType, Optional[bytes], Optional[bytes]
        ],
        req_attr: int = ALL_ATTRIBUTES,
    ) -> FileAttributeDataOrErrorV2:
        sha1 = None
        if req_attr & FileAttributes.SHA1_HASH:
            sha1 = Sha1OrError(sha1=expected_attributes[0])

        size = None
        if req_attr & FileAttributes.FILE_SIZE:
            size = SizeOrError(size=expected_attributes[1])

        sourceControlType = None
        if req_attr & FileAttributes.SOURCE_CONTROL_TYPE:
            sourceControlType = SourceControlTypeOrError(
                sourceControlType=expected_attributes[2]
            )

        objectId = None
        if req_attr & FileAttributes.OBJECT_ID:
            objectId = ObjectIdOrError(expected_attributes[3])

        blake3 = None
        if (req_attr & FileAttributes.BLAKE3_HASH) and expected_attributes[
            4
        ] is not None:
            blake3 = Blake3OrError(blake3=expected_attributes[4])

        return FileAttributeDataOrErrorV2(
            fileAttributeData=FileAttributeDataV2(
                sha1=sha1,
                size=size,
                sourceControlType=sourceControlType,
                objectId=objectId,
                blake3=blake3,
            )
        )

    def test_readdir(self) -> None:
        # each of these tests should arguably be their own test case,
        # but integration tests are expensive, so we will do it all in one.

        # non empty directories
        with self.get_thrift_client_legacy() as client:
            adir_result = DirListAttributeDataOrError(
                dirListAttributeData={
                    b"file": self.constructReaddirResult(
                        self.get_expected_file_attributes(
                            "adir/file", self.adir_file_id
                        )
                    )
                }
            )
            bdir_result = DirListAttributeDataOrError(
                dirListAttributeData={
                    b"file": self.constructReaddirResult(
                        self.get_expected_file_attributes(
                            "bdir/file", self.bdir_file_id
                        )
                    )
                }
            )

            expected = ReaddirResult([adir_result, bdir_result])
            actual_result = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b"adir", b"bdir"],
                    requestedAttributes=ALL_ATTRIBUTES,
                    sync=SyncBehavior(),
                )
            )
            self.assertEqual(
                expected,
                actual_result,
            )

            # empty directory
            # can't prep this before hand, because the initial setup if for the
            # backing repo, and we can not commit an empty directory, so it be added
            # via the backing repo.
            path = Path(self.mount) / "emptydir"
            os.mkdir(path)

            expected = ReaddirResult(
                [DirListAttributeDataOrError(dirListAttributeData={})]
            )
            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b"emptydir"],
                    sync=SyncBehavior(),
                )
            )
            self.assertEqual(expected, actual)

            # non existent directory
            expected = ReaddirResult(
                [
                    DirListAttributeDataOrError(
                        error=EdenError(
                            message="ddir: No such file or directory",
                            errorCode=2,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    )
                ]
            )
            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b"ddir"],
                    sync=SyncBehavior(),
                )
            )
            self.assertEqual(expected, actual)

            # file
            expected = ReaddirResult(
                [
                    DirListAttributeDataOrError(
                        error=EdenError(
                            message="hello: path must be a directory",
                            errorCode=22,
                            errorType=EdenErrorType.ARGUMENT_ERROR,
                        )
                    )
                ]
            )
            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b"hello"],
                    sync=SyncBehavior(),
                )
            )
            self.assertEqual(expected, actual)

            # empty string
            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b""],
                    sync=SyncBehavior(),
                )
            )
            # access the data to ensure this does not throw and we have legit
            # data in the response
            actual.dirLists[0].get_dirListAttributeData()
            self.assertIn(b"test_fetch1", actual.dirLists[0].get_dirListAttributeData())
            self.assertIn(b"hello", actual.dirLists[0].get_dirListAttributeData())
            self.assertIn(b"cdir", actual.dirLists[0].get_dirListAttributeData())

    def readdir_single_attr_only(self, req_attr: int) -> None:
        with self.get_thrift_client_legacy() as client:
            adir_result = DirListAttributeDataOrError(
                dirListAttributeData={
                    b"file": self.constructReaddirResult(
                        self.get_expected_file_attributes(
                            "adir/file", self.adir_file_id
                        ),
                        req_attr=req_attr,
                    )
                }
            )
            bdir_result = DirListAttributeDataOrError(
                dirListAttributeData={
                    b"file": self.constructReaddirResult(
                        self.get_expected_file_attributes(
                            "bdir/file", self.bdir_file_id
                        ),
                        req_attr=req_attr,
                    )
                }
            )

            expected = ReaddirResult([adir_result, bdir_result])
            actual_result = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [b"adir", b"bdir"],
                    requestedAttributes=req_attr,
                    sync=SyncBehavior(),
                )
            )
            print(expected)
            print(actual_result)

            self.assertEqual(
                expected,
                actual_result,
            )

    def test_readdir_single_attr_only(self) -> None:
        self.readdir_single_attr_only(FileAttributes.SHA1_HASH)

        self.readdir_single_attr_only(FileAttributes.BLAKE3_HASH)

        self.readdir_single_attr_only(FileAttributes.FILE_SIZE)

        self.readdir_single_attr_only(FileAttributes.SOURCE_CONTROL_TYPE)

    def readdir_no_size_or_sha1(
        self,
        parent_name: bytes,
        entry_name: bytes,
        error_message: str,
        error_code: int,
        source_control_type: SourceControlType,
        object_id: Optional[bytes],
    ) -> None:
        with self.get_thrift_client_legacy() as client:
            expected = FileAttributeDataOrErrorV2(
                fileAttributeData=FileAttributeDataV2(
                    sha1=Sha1OrError(
                        error=EdenError(
                            message=error_message,
                            errorCode=error_code,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                    size=SizeOrError(
                        error=EdenError(
                            message=error_message,
                            errorCode=error_code,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                    sourceControlType=SourceControlTypeOrError(
                        sourceControlType=source_control_type
                    ),
                    objectId=ObjectIdOrError(
                        objectId=object_id,
                    ),
                    blake3=Blake3OrError(
                        error=EdenError(
                            message=error_message,
                            errorCode=error_code,
                            errorType=EdenErrorType.POSIX_ERROR,
                        )
                    ),
                )
            )

            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [parent_name],
                    requestedAttributes=ALL_ATTRIBUTES,
                    sync=SyncBehavior(),
                )
            )
            print(expected)
            print(actual)

            self.assertEqual(
                expected,
                actual.dirLists[0].get_dirListAttributeData()[entry_name],
            )

            expected = FileAttributeDataOrErrorV2(
                fileAttributeData=FileAttributeDataV2(
                    sha1=None,
                    size=None,
                    sourceControlType=SourceControlTypeOrError(
                        sourceControlType=source_control_type,
                    ),
                    blake3=None,
                )
            )

            actual = client.readdir(
                ReaddirParams(
                    self.mount_path_bytes,
                    [parent_name],
                    requestedAttributes=FileAttributes.SOURCE_CONTROL_TYPE,
                    sync=SyncBehavior(),
                )
            )
            print(expected)
            print(actual)
            self.assertEqual(
                expected,
                actual.dirLists[0].get_dirListAttributeData()[entry_name],
            )

    def test_readdir_directory_symlink_and_other(self) -> None:
        self.readdir_no_size_or_sha1(
            parent_name=b"cdir",
            entry_name=b"subdir",
            error_message="cdir/subdir: Is a directory",
            error_code=21,
            source_control_type=SourceControlType.TREE,
            object_id=self.cdir_subdir_id,
        )
        if sys.platform != "win32":
            sockpath = self.get_path("adir/asock")
            # UDS are not supported in python on Win until 3.9:
            # https://bugs.python.org/issue33408
            with socket.socket(socket.AF_UNIX) as sock:
                sock.bind(sockpath)
                self.readdir_no_size_or_sha1(
                    parent_name=b"adir",
                    entry_name=b"asock",
                    error_message="adir/asock: file is a non-source-control type: 12: Invalid argument",
                    error_code=22,
                    source_control_type=SourceControlType.UNKNOWN,
                    object_id=None,
                )

        self.readdir_no_size_or_sha1(
            parent_name=b"",
            entry_name=b"slink",
            error_message="slink: file is a symlink: Invalid argument",
            error_code=22,
            source_control_type=SourceControlType.SYMLINK,
            object_id=self.slink_id,
        )

    def test_materialized_files_return_no_object_id(self) -> None:
        self.write_file("adir/file", "new contents\n")

        actual = self.get_attributes_v2([b"adir/file"], FileAttributes.OBJECT_ID)
        self.assertEqual(1, len(actual.res))
        expected = FileAttributeDataOrErrorV2(
            fileAttributeData=FileAttributeDataV2(objectId=ObjectIdOrError(None))
        )
        self.assertEqual(expected, actual.res[0])
