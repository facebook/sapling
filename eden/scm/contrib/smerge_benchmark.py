# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time
from dataclasses import dataclass

from edenscm import commands, registrar
from edenscm.simplemerge import Merge3Text, wordmergemode


cmdtable = {}
command = registrar.command(cmdtable)


class SmartMerge3Text(Merge3Text):
    def __init__(self, basetext, atext, btext, wordmerge=wordmergemode.ondemand):
        # a dummy implementation by enabling wordmerge in `Merge3Text`
        Merge3Text.__init__(self, basetext, atext, btext, wordmerge=wordmerge)


@dataclass
class BenchStats:
    changed_files: int = 0
    unresolved_files: int = 0
    unmatched_files: int = 0
    octopus_merges: int = 0
    criss_cross_merges: int = 0


@command("smerge_bench", commands.dryrunopts)
def smerge_bench(ui, repo, **opts):
    merge_commits = repo.dageval(lambda dag: dag.merges(dag.all()))
    ui.write(f"len(merge_commits)={len(merge_commits)}\n")

    for m3merger in [Merge3Text, SmartMerge3Text]:
        ui.write(f"\n============== {m3merger.__name__} ==============\n")
        start = time.time()
        bench_stats = BenchStats()
        for i, merge_commit in enumerate(merge_commits, start=1):
            parents = repo.dageval(lambda: parentnames(merge_commit))

            if len(parents) != 2:
                # skip octopus merge
                #    a
                #  / | \
                # b  c  d
                #  \ | /
                #    e
                bench_stats.octopus_merges += 1
                continue

            p1, p2 = parents
            gcas = repo.dageval(lambda: gcaall([p1, p2]))
            if len(gcas) != 1:
                # skip criss cross merge
                #    a
                #   / \
                #  b1  c1
                #  |\ /|
                #  | X |
                #  |/ \|
                #  b2  c2
                bench_stats.criss_cross_merges += 1
                continue

            basectx = repo[gcas[0]]
            p1ctx, p2ctx = repo[p1], repo[p2]
            mergectx = repo[merge_commit]

            for filepath in mergectx.files():
                if all(filepath in ctx for ctx in [basectx, p1ctx, p2ctx]):
                    merge_file(
                        repo,
                        p1ctx,
                        p2ctx,
                        basectx,
                        mergectx,
                        filepath,
                        m3merger,
                        bench_stats,
                    )

            if i % 100 == 0:
                ui.write(f"{i} {bench_stats}\n")

        ui.write(f"\nSummary: {bench_stats}\n")
        ui.write(f"Execution time: {time.time() - start:.2f} seconds\n")


def merge_file(
    repo, dstctx, srcctx, basectx, mergectx, filepath, m3merger, bench_stats
):
    srctext = srcctx[filepath].data()
    dsttext = dstctx[filepath].data()
    basetext = basectx[filepath].data()

    if srctext == dsttext:
        return

    bench_stats.changed_files += 1

    m3 = m3merger(basetext, dsttext, srctext)
    # merge_lines() has side effect setting conflictscount
    mergedtext = b"".join(m3.merge_lines())

    if m3.conflictscount:
        bench_stats.unresolved_files += 1
    elif mergedtext != mergectx[filepath].data():
        bench_stats.unmatched_files += 1
