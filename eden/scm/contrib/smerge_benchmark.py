# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass

from edenscm import commands, registrar
from edenscm.simplemerge import Merge3Text, wordmergemode


cmdtable = {}
command = registrar.command(cmdtable)


class SmartMerge3Text:
    def __init__(self, basetext, atext, btext, wordmerge=False):
        pass


@dataclass
class BenchStats:
    changed_files: int = 0
    unresolved_files: int = 0
    unmatched_files: int = 0


@command("smerge_bench", commands.dryrunopts)
def smerge_bench(ui, repo, **opts):
    merge_commits = repo.dageval(lambda: merges(all()))  # noqa
    ui.write(f"len(merge_commits)={len(merge_commits)}\n")

    bench_stats = BenchStats()
    for i, merge_commit in enumerate(merge_commits, start=1):
        mergectx = repo[merge_commit]
        p1, p2 = mergectx.p1(), mergectx.p2()
        base = repo.dageval(lambda: gcaone([p1.node(), p2.node()]))
        basectx = repo[base]

        for filepath in mergectx.files():
            if all(filepath in ctx for ctx in [basectx, p1, p2]):
                merge_file(repo, p1, p2, basectx, mergectx, filepath, bench_stats)

        if i % 100 == 0:
            ui.write(f"{i} {bench_stats}\n")
    ui.write(f"{bench_stats}\n")


def merge_file(repo, dstctx, srcctx, basectx, mergectx, filepath, bench_stats):
    srctext = srcctx[filepath].data()
    dsttext = dstctx[filepath].data()
    basetext = basectx[filepath].data()

    if srctext == dsttext:
        return

    bench_stats.changed_files += 1

    m3 = Merge3Text(basetext, dsttext, srctext, wordmerge=wordmergemode.disabled)
    # merge_lines() has side effect setting conflictscount
    mergedtext = b"".join(m3.merge_lines())

    if m3.conflictscount:
        bench_stats.unresolved_files += 1
    elif mergedtext != mergectx[filepath].data():
        bench_stats.unmatched_files += 1
