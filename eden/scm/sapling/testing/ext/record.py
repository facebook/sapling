# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""record - track test states for each step"""

import functools
import hashlib
import json
import os
import shlex
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


def restore_state_script(metalog) -> str:
    """Attempt to restore the testing environemnt: files, env, cwd.
    The env and cwd cannot be restored to the current shell. So a shell script
    is written to set the env and cwd. Returns a path to the script.
    """
    modes = json.loads(metalog["modes"].decode())
    env = json.loads(metalog["env"].decode())
    testtmp = metalog["TESTTMP"].decode()
    os.makedirs(testtmp, exist_ok=True)

    # Remove files that are not tracked.
    all_paths = set(_walk(testtmp))
    deleted_paths = all_paths - set(modes)
    for path in sorted(deleted_paths, reverse=True):
        # For convenience, do not delete top-level hidden files.
        # This allows .git for manual investigation (ex. git add . and check
        # differences), or keep shell rc and history.
        if path.startswith("."):
            continue
        fullpath = os.path.join(testtmp, path)
        try:
            if os.path.isdir(fullpath):
                os.rmdir(fullpath)
            else:
                os.unlink(fullpath)
        except OSError:
            pass

    # Write out tracked files.
    for key in metalog.keys():
        if not key.startswith(b"file:"):
            continue
        path = key.split(b":", 1)[-1].decode()
        data = metalog[key.decode()]
        fullpath = os.path.join(testtmp, path)
        mode = modes[path]
        os.makedirs(os.path.dirname(fullpath), exist_ok=True)
        if os.path.isdir(fullpath):
            os.rmdir(fullpath)
        if stat.S_ISLNK(mode):
            try:
                os.symlink(data.decode(), fullpath)
            except FileExistsError:
                os.unlink(fullpath)
                os.symlink(data.decode(), fullpath)
        elif stat.S_ISREG(mode):
            with open(fullpath, "wb") as f:
                f.write(data)
            os.chmod(fullpath, mode)

    # Write a script to set the env variables. The script can be executed or sourced.
    script_path = os.path.join(testtmp, "env.sh")
    cwd = metalog["cwd"].decode()
    env = json.loads(metalog["env"].decode())
    eol = "\n"  # workaround '\' not allowed in f-strings
    ps1 = ""
    for line in metalog.message().splitlines():
        if line.startswith("Line "):
            ps1 += f"line {int(line.split(' ', 1)[-1]) + 1}"
        elif line.startswith("Test "):
            ps1 += line.split(" ", 1)[-1] + " "
    with open(script_path, "wb") as f:
        f.write(
            f"""#!/bin/bash
ORIG_PATH="$PATH"
{''.join(f"export {name}={shlex.quote(value)}{eol}" for name, value in env.items())}
cd {shlex.quote(os.path.join(testtmp, cwd))}
if [[ $- != *i* ]] && [[ -n "$TESTTMP" ]]; then
    if [[ $TERM = fake-term ]]; then
        export TERM=xterm-256color
    fi
    echo 1>&2 "Entering ({ps1}) testing environment".
    export PS1="[\\W ({ps1.strip()})]\\$ "
    # provide the original PATH for convenience
    export PATH="$PATH:$ORIG_PATH"
    /bin/bash --norc --noprofile -i
    echo 1>&2 "Exiting ({ps1}) testing environment".
fi
""".encode()
        )
    os.chmod(script_path, os.stat(script_path).st_mode | stat.S_IXUSR)
    return script_path


def try_locate_metalog(test_filename: str, testcase=None, loc: int = 0):
    """Given the test file and line number, locate a matching metalog state.
    Returns the metalog state, or None if no such record is found.
    """
    metalog = _get_record_metalog(test_filename, testcase, create=False)
    if metalog is None:
        return None
    best = None
    for root in metalog.roots():  # oldest -> newest
        current_metalog = metalog.checkout(root)
        msg = current_metalog.message()
        for line in msg.splitlines():
            if line.startswith("Line "):
                current_loc = int(line.split()[-1])  # starts from 0
                if current_loc <= loc:
                    best = current_metalog
                else:
                    return best
    return best


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
