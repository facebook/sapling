# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Creates two long branches with a common ancestor in Git and Mercurial.
Measures time needed to calculate the common ancestor (merge base) of
the two branches.

Note: This script only works with Mercurial (not Sapling). If you work
at Meta you'll need to the `HG` environment variable to an executable
that runs the external Mercurial.
"""

import hashlib
import os
import struct
from binascii import hexlify
from subprocess import PIPE, Popen, run

# 5 million commits in total.
# Note this can take 6 minutes and 1.7GB space to run.
N = 5_000_000

os.makedirs("long-branch-gca", exist_ok=True)
os.chdir("long-branch-gca")

# Git repo
git = [os.environ.get("GIT") or "git", "--git-dir=git-repo/.git"]
if not os.path.exists("git-repo"):
    print(f"Creating git-repo with {N} commits")
    run(git[:1] + ["-c", "init.defaultBranch=main", "init", "-q", "git-repo"])
    ps = Popen(git + ["fast-import", "--quiet"], stdin=PIPE)
    progress = 1
    for i in range(1, N + 1):
        parent = i > 1 and f"from :{i - 1 == N // 2 and 1 or (i - 1)}\n" or ""
        msg = str(i)
        data = (
            f"commit refs/heads/{i - 1 < N // 2 and 'A' or 'B'}\n"
            f"mark :{i}\n"
            f"committer user <user@example.com> {i - 1} +0000\n"
            f"data {len(msg)}\n"
            f"{msg}\n"
            f"{parent}deleteall\n\n"
        ).encode()
        ps.stdin.write(data)
        if i == N * progress // 10:
            print(f"{progress * 10}%")
            progress += 1
    ps.stdin.close()
    ps.wait()
    print("Creating git commit graph index")
    run(git + ["config", "--local", "core.commitGraph", "true"])
    run(git + ["commit-graph", "write"])


# Hg repo
hg = [os.environ.get("HG") or "hg", "--cwd=hg-repo"]
if not os.path.exists("hg-repo"):
    print(f"Creating hg-repo with {N} commits")
    run(hg[:1] + ["init", "hg-repo"])
    # Similar to: run(hg + ["debugbuilddag", f"+{N//2}:A*{N//2}+{N//2}:B"])
    # but much faster than debugbuilddag.
    cl = "hg-repo/.hg/store/00changelog"
    with open(f"{cl}.i", "wb") as ifp, open(f"{cl}.d", "wb") as dfp:
        nullid = b"\0" * 20
        nodes = [nullid]
        ipack = struct.Struct(">Qiiiiii20s12x").pack
        dlen = 0
        progress = 1
        for i in range(0, N):
            prev = 0 if i == N // 2 else i - 1
            pnode = nodes[prev + 1]
            msg = hexlify(nullid) + f"\nuser\n{i + 1} 0\n\n{i + 1}".encode()
            node = hashlib.sha1(nullid + pnode + msg).digest()
            ddata = b"u" + msg
            idata = ipack(dlen << 16, len(ddata), len(msg), i, i, prev, -1, node)
            if i == 0:
                idata = b"\0\0\0\1" + idata[4:]
            ifp.write(idata)
            dfp.write(ddata)
            dlen += len(ddata)
            nodes.append(node)
            if i + 1 == N * progress // 10:
                print(f"{progress * 10}%")
                progress += 1
    with open("hg-repo/.hg/localtags", "wb") as f:
        f.write(hexlify(nodes[N // 2]) + b" A\n")
        f.write(hexlify(nodes[-1]) + b" B\n")
    # Warm up .hg/cache/rbc-revs-v1, used to convert hashes to rev num.
    # This can take a while and is not needed if we use revision numbers in the
    # gca command below, f"ancestor({N//2-1},{N-1})".
    # We don't use debugupdatecaches here because it updates other caches like
    # the tag cache which scans `.hgtags` for every commit, and is too slow.
    print("Creating hg nodemap cache")
    run(hg + ["log", "-r", b"+".join(map(hexlify, nodes[:10])).decode(), "-T{x}"])


print("\n---- Time for gca(A,B) ----")
print("\nGit with commit-graph:")
run(["time", "-p"] + git + ["merge-base", "A", "B"])

print("\nMercurial excluding Python startup:")
run(hg + ["--time", "log", "-r", "ancestor(A,B)", "-T{node}\n"])

# Example output (git 2.30.2, hg 6.2.1):
#
# ---- Time for gca(A,B) ----
#
# Git with commit-graph:
# 32eec408869b1672aebcb75811ddc760422edbd3
# real 8.27
# user 7.47
# sys 0.79
#
# Mercurial excluding Python startup:
# cc74063e331f486fd523ff9290b4789ddc95b949
# time: real 0.580 secs (user 0.250+0.000 sys 0.330+0.010)
