# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time
from dataclasses import dataclass

from edenscm import commands, error, mdiff, registrar
from edenscm.i18n import _
from edenscm.simplemerge import Merge3Text, wordmergemode


cmdtable = {}
command = registrar.command(cmdtable)


class SmartMerge3Text(Merge3Text):
    """
    SmergeMerge3Text uses vairable automerge algorithms to resolve conflicts.
    """

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


@command(
    "sresolve",
    [
        (
            "s",
            "smart",
            None,
            _("use the smart merge for resolving conflicts"),
        ),
        (
            "o",
            "output",
            "/tmp/sresolve.txt",
            _("output file path of the resolved text"),
        ),
    ],
    _("[OPTION]... <FILEPATH> <DEST> <SRC> <BASE>"),
)
def sresolve(ui, repo, *args, **opts):
    """
    sresolve resolves file conficts based on the specified dest, src and base revisions.

    This is for manually verifying the correctness of merge conflict resolution. The input
    arguments order `<FILEPATH> <DEST> <SRC> <BASE>` matches the output of `smerge_bench`
    command.
    """
    if len(args) != 4:
        raise error.CommandError("smerge", _("invalid arguments"))

    filepath, dest, src, base = args
    desttext = repo[dest][filepath].data()
    srctext = repo[src][filepath].data()
    basetext = repo[base][filepath].data()

    if opts.get("smart"):
        m3 = SmartMerge3Text(basetext, desttext, srctext)
    else:
        m3 = Merge3Text(basetext, desttext, srctext)

    extrakwargs = {}
    extrakwargs["base_marker"] = b"|||||||"
    extrakwargs["name_base"] = b"base"
    extrakwargs["minimize"] = False

    mergedtext = b"".join(
        m3.merge_lines(name_a=b"dest", name_b=b"src", **extrakwargs)
    ).decode("utf8")

    if output := opts.get("output"):
        with open(output, "wb") as f:
            f.write(mergedtext.encode("utf8"))
    else:
        ui.write(mergedtext)


@command("smerge_bench", commands.dryrunopts)
def smerge_bench(ui, repo, **opts):
    merge_commits = repo.dageval(lambda dag: dag.merges(dag.all()))
    ui.write(f"len(merge_commits)={len(merge_commits)}\n")

    for m3merger in [SmartMerge3Text, Merge3Text]:
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
    else:
        expectedtext = mergectx[filepath].data()
        if mergedtext != expectedtext:
            bench_stats.unmatched_files += 1
            mergedtext_baseline = b""

            if m3merger != Merge3Text:
                m3_baseline = Merge3Text(basetext, dsttext, srctext)
                mergedtext_baseline = b"".join(m3_baseline.merge_lines())

            if mergedtext != mergedtext_baseline:
                repo.ui.write(
                    f"\nUnmatched_file: {filepath} {dstctx} {srcctx} {basectx} {mergectx}\n"
                )
                difftext = unidiff(mergedtext, expectedtext, filepath).decode("utf8")
                repo.ui.write(f"{difftext}\n")


def unidiff(atext, btext, filepath="") -> bytes:
    """
    generate unified diff between two texts.

    >>> basetext = b"a\\nb\\nc\\n"
    >>> atext = b"a\\nd\\nc\\n"
    >>> print(unidiff(basetext, atext).decode("utf8")) # doctest: +NORMALIZE_WHITESPACE
    --- a/
    +++ b/
    @@ -1,3 +1,3 @@
     a
    -b
    +d
     c
    """
    headers, hunks = mdiff.unidiff(atext, "", btext, "", filepath, filepath)
    result = headers
    for hunk in hunks:
        result.append(b"".join(hunk[1]))
    return b"\n".join(result)


if __name__ == "__main__":
    import doctest

    doctest.testmod()
