# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""record - track test states for each step"""

import functools
import hashlib
import json
import os
import shutil
import stat

# Usually, "testing" modules do not depend on sapling-only logic.
# However, this is an extension. We want to "source control" the TESTTMP which
# might contain ".sl" and ".git". Metalog is a good fit.
import bindings

# Calculate cache_dir() before $HOME gets changed to $TESTTMP.
user_cache_dir = bindings.dirs.cache_dir()

_metalog_cache = {}

dumps = functools.partial(json.dumps, indent=2, sort_keys=True)


def _get_record_metalog(test_filename: str, testcase=None, create=False):
    cache_key = (test_filename, testcase)
    cached = _metalog_cache.get(cache_key)
    if cached is not None:
        return cached
    with open(test_filename, "rb") as f:
        hash = hashlib.blake2b(f.read()).hexdigest()[:10]
    name = os.path.basename(test_filename)
    if testcase is not None:
        name += f"-{testcase}"
    # ex. ~/.cache/Sapling/TestRecord/test-a.t/f4c0f2b4a2
    metalog_path = os.path.join(
        user_cache_dir,
        "Sapling",
        "TestRecord",
        name,
        hash,
    )
    if create and os.path.isdir(metalog_path):
        shutil.rmtree(metalog_path)
        _get_record_metalog.cache_clear()
    elif not create and not os.path.isdir(metalog_path):
        return None
    metalog = bindings.metalog.metalog(metalog_path)
    _metalog_cache[cache_key] = metalog
    return metalog


def _walk(top: str):
    """Find all paths in `top` recursively. Yield `path_rel_to_top: str`"""
    # Cannot use glob.glob as it does not show hidden files (until Python 3.11)
    for root, dirs, files in os.walk(top):
        rel_root = os.path.relpath(root, top)
        for dir in dirs:
            yield os.path.join(rel_root, dir)
        for file in files:
            yield os.path.join(rel_root, file)


def save_state(metalog, env, cwd: str, root: str):
    """Update metalog to store the env, cwd, and all paths in "root" path.
    Does not make a commit in the metalog.
    """
    metalog["env"] = dumps(env).encode()
    metalog["cwd"] = cwd.encode()
    all_paths = sorted(_walk(root))
    all_keys = set()
    modes = {}
    for path in all_paths:
        fullpath = os.path.join(root, path)
        mode = os.lstat(path).st_mode
        modes[path] = mode
        if stat.S_ISLNK(mode):
            data = os.readlink(fullpath).encode()
        elif stat.S_ISREG(mode):
            with open(fullpath, "rb") as f:
                data = f.read()
        else:
            continue
        key = f"file:{path}"
        metalog[key] = data
        all_keys.add(key)
    for deleted_path in {
        k.decode() for k in metalog.keys() if k.startswith(b"file:")
    } - all_keys:
        metalog.remove(deleted_path)
    metalog["modes"] = dumps(modes).encode()


def testsetup(t):
    class TestTmpRecord(t.__class__):
        def post_checkoutput(
            self,
            a: str,
            b: str,
            src: str,
            srcloc: int,
            outloc: int,
            endloc: int,
            indent: int,
            filename: str,
        ):
            metalog = _get_record_metalog(filename, self._testcase, create=True)
            shenv = self.shenv
            root = str(self.path)
            save_state(
                metalog, env=shenv.getexportedenv(), cwd=shenv.fs.cwd(), root=root
            )
            metalog["TESTTMP"] = root.encode()
            testname = os.path.basename(filename)
            if self._testcase is not None:
                testname += f":{self._testcase}"
            metalog.commit(f"After {src}\nTest {testname}\nLine {srcloc}")

            return super().post_checkoutput(
                a, b, src, srcloc, outloc, endloc, indent, filename
            )

    t.__class__ = TestTmpRecord
