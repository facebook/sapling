# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time
from dataclasses import dataclass

from edenscm import commands, error, mdiff, registrar, scmutil
from edenscm.i18n import _
from edenscm.simplemerge import Merge3Text, wordmergemode

cmdtable = {}
command = registrar.command(cmdtable)

A, B, BASE = range(3)


class Conflict:
    def __init__(self, conflict_region):
        _, z1, z2, a1, a2, b1, b2 = conflict_region
        self.start = [a1, b1, z1]
        self.end = [a2, b2, z2]
        self.merged_lines = []

    def __str__(self):
        return f"{list(zip(self.start, self.end))}"


class SmartMerge3Text(Merge3Text):
    """
    SmergeMerge3Text uses vairable automerge algorithms to resolve conflicts.
    """

    def __init__(self, basetext, atext, btext, wordmerge=wordmergemode.disabled):
        Merge3Text.__init__(self, basetext, atext, btext, wordmerge=wordmerge)

    def resolve_conflict_region(self, conflict_region):
        """Try automerge algorithms to resolve the conflict region.

        Return resolved lines, or None if auto resolution failed.
        """
        c = Conflict(conflict_region)

        if self.merge_adjacent_changes(c):
            return c.merged_lines

        return None

    def merge_adjacent_changes(self, c: Conflict) -> bool:
        # require something to be changed
        if c.start[BASE] == c.end[BASE]:
            return False

        base_lines = self.base[c.start[BASE] : c.end[BASE]]
        a_lines = self.a[c.start[A] : c.end[A]]
        b_lines = self.b[c.start[B] : c.end[B]]

        ablocks = unmatching_blocks(base_lines, a_lines)
        bblocks = unmatching_blocks(base_lines, b_lines)

        k = c.start[BASE]
        indexes = [0, 0]
        while indexes[A] < len(ablocks) and indexes[B] < len(bblocks):
            ablock, bblock = ablocks[indexes[A]], bblocks[indexes[B]]
            if is_overlap(ablock[0], ablock[1], bblock[0], bblock[1]):
                c.merged_lines = []
                return False
            elif is_non_unique_separator_for_insertion(
                base_lines, a_lines, b_lines, ablock, bblock
            ):
                c.merged_lines = []
                return False

            i, block, lines = (
                (A, ablock, a_lines) if ablock[0] < bblock[0] else (B, bblock, b_lines)
            )
            # add base lines before the block
            while k < block[0]:
                c.merged_lines.append(self.base[k])
                k += 1
            # skip base lines being deleted
            k += block[1] - block[0]
            # add new lines from the block
            c.merged_lines += lines[block[2] : block[3]]

            indexes[i] += 1

        if indexes[A] < len(ablocks):
            block, lines = ablocks[indexes[A]], a_lines
        else:
            block, lines = bblocks[indexes[B]], b_lines

        while k < block[0]:
            c.merged_lines.append(self.base[k])
            k += 1
        k += block[1] - block[0]
        c.merged_lines += lines[block[2] : block[3]]

        # add base lines at the end of block
        c.merged_lines += self.base[k : c.end[BASE]]

        return True


def is_non_unique_separator_for_insertion(
    base_lines, a_lines, b_lines, ablock, bblock
) -> bool:
    def is_sub_list(list1, list2):
        "check if list1 is a sublist of list2"
        # PERF: might be able to use rolling hash to optimize the time complexity
        len1, len2 = len(list1), len(list2)
        if len1 > len2:
            return False
        for i in range(len2 - len1 + 1):
            if list1 == list2[i : i + len1]:
                return True
        return False

    # no insertion on both sides
    if not (ablock[0] == bblock[0] or ablock[1] == bblock[1]):
        return False

    if ablock[1] <= bblock[0]:
        base_start, base_end = ablock[1], bblock[0]
    else:
        base_start, base_end = bblock[1], ablock[0]

    if base_start <= base_end:
        # empty is a subset of any list
        return True

    base_list = base_lines[base_start:base_end]
    a_list = a_lines[ablock[3] : ablock[4]]
    b_list = b_lines[bblock[3] : bblock[4]]
    return is_sub_list(base_list, a_list) or is_sub_list(base_list, b_list)


@dataclass
class BenchStats:
    changed_files: int = 0
    unresolved_files: int = 0
    unmatched_files: int = 0
    octopus_merges: int = 0
    criss_cross_merges: int = 0


@command(
    "debugsmerge",
    [],
    _("[OPTION]... <DEST_FILE> <SRC_FILE> <BASE_FILE>"),
)
def debugsmerge(ui, repo, *args, **opts):
    """
    debug the performance of SmartMerge3Text
    """
    if len(args) != 3:
        raise error.CommandError("debugsmerge", _("invalid arguments"))

    desttext, srctext, basetext = [readfile(p) for p in args]
    m3 = SmartMerge3Text(basetext, desttext, srctext)
    lines = merge_lines(m3)
    mergedtext = b"".join(lines)
    ui.fout.write(mergedtext)


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

    filepath = args[0]
    dest, src, base = [scmutil.revsingle(repo, x) for x in args[1:]]

    desttext = repo[dest][filepath].data()
    srctext = repo[src][filepath].data()
    basetext = repo[base][filepath].data()

    if opts.get("smart"):
        m3 = SmartMerge3Text(basetext, desttext, srctext)
    else:
        m3 = Merge3Text(basetext, desttext, srctext)

    mergedtext = b"".join(merge_lines(m3))

    if output := opts.get("output"):
        with open(output, "wb") as f:
            f.write(mergedtext)
    else:
        ui.fout.write(mergedtext)


def merge_lines(m3, name_a=b"dest", name_b=b"source"):
    extrakwargs = {}
    extrakwargs["base_marker"] = b"|||||||"
    extrakwargs["name_base"] = b"base"
    extrakwargs["minimize"] = False

    return m3.merge_lines(name_a=b"dest", name_b=b"src", **extrakwargs)


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


def basediff(m3, name_a, name_b):
    lines = []
    conflicts = False
    for group in m3.merge_groups():
        if group[0] == "conflict":
            base_lines, a_lines, b_lines = group[1:]
            basetext = b"".join(base_lines)
            bblocks = list(
                mdiff.allblocks(
                    basetext,
                    b"".join(b_lines),
                    lines1=base_lines,
                    lines2=b_lines,
                )
            )
            ablocks = list(
                mdiff.allblocks(
                    basetext,
                    b"".join(a_lines),
                    lines1=base_lines,
                    lines2=b_lines,
                )
            )

            def difflines(blocks, lines1, lines2):
                for block, kind in blocks:
                    if kind == "=":
                        for line in lines1[block[0] : block[1]]:
                            yield b" " + line
                    else:
                        for line in lines1[block[0] : block[1]]:
                            yield b"-" + line
                        for line in lines2[block[2] : block[3]]:
                            yield b"+" + line

            lines.append(b"<<<<<<< %s\n" % name_a)
            lines.extend(difflines(ablocks, base_lines, a_lines))
            lines.append(b"=======\n")
            lines.extend(difflines(bblocks, base_lines, b_lines))
            lines.append(b">>>>>>> %s\n" % name_b)
            conflicts = True
        else:
            lines.extend(group[1])
    return lines, conflicts


def readfile(path):
    with open(path, "rb") as f:
        return f.read()


def unmatching_blocks(lines1, lines2):
    text1 = b"".join(lines1)
    text2 = b"".join(lines2)
    blocks = mdiff.allblocks(text1, text2, lines1=lines1, lines2=lines2)
    return [b[0] for b in blocks if b[1] == "!"]


def is_overlap(s1, e1, s2, e2):
    return not (s1 >= e2 or s2 >= e1)


if __name__ == "__main__":
    import doctest

    doctest.testmod()
