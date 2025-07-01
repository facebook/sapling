# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
This script is for evaluating the performance of automerge (smart merge) algorithms.

The script supports both git repositories and internal sapling repositories. Below
are the steps to use this script for the `git/git` repository, since it has many merge
commits and it's faster to run the script on a local git repository.

1. Clone the `git/git` repository

```
$ sl clone https://github.com/git/git.git
```

2. Run following command to evaluate the performance of automerge algorithms

```
$ sl smerge_bench --algos=adjacent-changes,subset-changes,word-merge -q > ~/data/git_automerge_wordmerge.txt
$ tail -2 ~/data/git_automerge_wordmerge.txt
Summary: BenchStats(merger_name="Merge3Text(algos=('adjacent-changes', 'subset-changes', 'word-merge'))", changed_files=26177, unresolved_files=1538, unmatched_files=251)
Execution time: 277.87 seconds

# baseline
$ sl smerge_bench -q > ~/data/git_no_automerge.txt
$ tail -2 ~/data/git_no_automerge.txt
Summary: BenchStats(merger_name='Merge3Text(algos=())', changed_files=26177, unresolved_files=2438, unmatched_files=102)
Execution time: 273.72 seconds
```

3. Analyze the results

Below is an example of a "bad" automerge result:

```
Unmatched_file: commit.h 2a2ad0c0007b 6bf4f1b4c9d7 a0b54e7b7341 267123b4299e
--- a/commit.h
+++ b/commit.h
@@ -80,7 +80,7 @@
       const char *subject,
       const char *after_subject,
       const char *encoding,
-      int plain_non_ascii);
+      int need_8bit_cte);
 void pp_remainder(enum cmit_fmt fmt,
      const char **msg_p,
      struct strbuf *sb,
```

then we can run below command to see the conflict

```
$ sl sresolve commit.h 2a2ad0c0007b 6bf4f1b4c9d7 a0b54e7b7341
writing to file: /tmp/sresolve.txt
$ open the file /tmp/sresolve.txt and search conflict markers
...
<<<<<<< dest: 2a2ad0c0007b - Merge branch 'maint'
        int non_ascii_present);
+void pp_user_info(const char *what, enum cmit_fmt fmt, struct strbuf *sb,
+      const char *line, enum date_mode dmode,
+      const char *encoding);
+void pp_title_line(enum cmit_fmt fmt,
+      const char **msg_p,
+      struct strbuf *sb,
+      const char *subject,
+      const char *after_subject,
+      const char *encoding,
+      int plain_non_ascii);
+void pp_remainder(enum cmit_fmt fmt,
+     const char **msg_p,
+     struct strbuf *sb,
+     int indent);
+
======= base: a0b54e7b7341 - Make man page building quiet when DOCBOOK_XSL_172 is defined
-       int non_ascii_present);
+       int need_8bit_cte);
>>>>>>> source: 6bf4f1b4c9d7 - format-patch: generate MIME header as needed even when there is format.header
```
"""

import csv
import re
import time
from dataclasses import dataclass

from sapling import error, mdiff, registrar, scmutil
from sapling.i18n import _
from sapling.simplemerge import Merge3Text, render_mergediffs, render_minimized

cmdtable = {}
command = registrar.command(cmdtable)


WHITE_SPACE_PATTERN = re.compile(b"\\s+")


def gen_3way_merger(ui, basetext, atext, btext, filepath, algos=()):
    ui.setconfig("automerge", "mode", "accept")
    ui.setconfig("automerge", "merge-algos", ",".join(algos))
    return Merge3Text(basetext, atext, btext, ui=ui, file_path=filepath)


@dataclass
class BenchStats:
    merger_name: str = ""
    changed_files: int = 0
    unresolved_files: int = 0
    unmatched_files: int = 0


@command(
    "debugsmerge",
    [
        (
            "",
            "algos",
            "",
            _("automerge algorithms (e.g.: 'adjacent-changes,subset-changes')."),
        )
    ],
    _("[OPTION]... <DEST_FILE> <SRC_FILE> <BASE_FILE>"),
)
def debugsmerge(ui, repo, *args, **opts):
    """
    debug the performance of SmartMerge3Text
    """
    if len(args) != 3:
        raise error.CommandError("debugsmerge", _("invalid arguments"))

    algos = str_to_tuple(opts.get("algos"))
    desttext, srctext, basetext = [readfile(p) for p in args]
    m3 = gen_3way_merger(ui, basetext, desttext, srctext, args[-1], algos)
    lines = render_mergediffs(m3, b"dest", b"source", b"base")[0]
    mergedtext = b"".join(lines)
    ui.fout.write(mergedtext)


@command(
    "sresolve",
    [
        (
            "",
            "algos",
            "",
            _("automerge algorithms (e.g.: 'adjacent-changes,subset-changes')."),
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
    sresolve resolves file conflicts based on the specified dest, src and base revisions.

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
    algos = str_to_tuple(opts.get("algos"))

    m3 = gen_3way_merger(ui, basetext, desttext, srctext, filepath, algos)

    def gen_label(prefix, ctx):
        s = f"{prefix}: {ctx} - " + ctx.description().split("\n")[0]
        return s.encode("utf-8")

    label_dest = gen_label("dest", dest)
    label_source = gen_label("source", src)
    label_base = gen_label("base", base)

    mergedtext = b"".join(
        render_mergediffs(m3, label_dest, label_source, label_base)[0]
    )

    if output := opts.get("output"):
        ui.write(f"writing to file: {output}\n")
        with open(output, "wb") as f:
            f.write(mergedtext)
    else:
        ui.fout.write(mergedtext)


@command(
    "smerge_bench",
    [
        ("f", "file", "", _("a file that contains merge commits (csv file).")),
        (
            "",
            "algos",
            "",
            _("automerge algorithms (e.g.: 'adjacent-changes,subset-changes')."),
        ),
    ],
)
def smerge_bench(ui, repo, *args, **opts):
    path = opts.get("file")
    if path:
        merge_ctxs = get_merge_ctxs_from_file(ui, repo, path)
    else:
        merge_ctxs = get_merge_ctxs_from_repo(ui, repo)

    algos = str_to_tuple(opts.get("algos"))
    m3merger_name = f"Merge3Text(algos={algos})"
    ui.write(f"\n============== {m3merger_name} ==============\n")
    start = time.time()
    bench_stats = BenchStats(m3merger_name)

    for i, (p1ctx, p2ctx, basectx, mergectx) in enumerate(merge_ctxs, start=1):
        for filepath in mergectx.files():
            if all(filepath in ctx for ctx in [basectx, p1ctx, p2ctx, mergectx]):
                merge_file(
                    repo,
                    p1ctx,
                    p2ctx,
                    basectx,
                    mergectx,
                    filepath,
                    algos,
                    bench_stats,
                )

        if i % 100 == 0:
            ui.note(f"{i} {bench_stats}\n")

    ui.write(f"\nSummary: {bench_stats}\n")
    ui.write(f"Execution time: {time.time() - start:.2f} seconds\n")


def get_merge_ctxs_from_repo(ui, repo):
    ui.write("generating merge data from repo ...\n")
    merge_commits = repo.dageval(lambda dag: dag.merges(dag.all()))
    octopus_merges, criss_cross_merges = 0, 0

    ctxs = []
    for i, merge_commit in enumerate(merge_commits, start=1):
        parents = repo.dageval(lambda: parentnames(merge_commit))
        if len(parents) != 2:
            # skip octopus merge
            #    a
            #  / | \
            # b  c  d
            #  \ | /
            #    e
            octopus_merges += 1
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
            criss_cross_merges += 1
            continue

        basectx = repo[gcas[0]]
        p1ctx, p2ctx = repo[p1], repo[p2]
        mergectx = repo[merge_commit]
        ctxs.append((p1ctx, p2ctx, basectx, mergectx))

    ui.write(
        f"len(merge_ctxs)={len(ctxs)}, octopus_merges={octopus_merges}, "
        f"criss_cross_merges={criss_cross_merges}\n"
    )
    return ctxs


def get_merge_ctxs_from_file(ui, repo, filepath):
    def get_merge_commits_from_file(filepath):
        merge_commits = []
        with open(filepath) as f:
            reader = csv.DictReader(f)
            for row in reader:
                merge_commits.append(
                    (row["dest_hex"], row["src_hex"], row["newnode_hex"])
                )
        return merge_commits

    def prefetch_commits(repo, commit_hashes):
        size = 1000
        chunks = [
            commit_hashes[i : i + size] for i in range(0, len(commit_hashes), size)
        ]
        n = len(chunks)
        for i, chunk in enumerate(chunks, start=1):
            ui.write(f"{int(time.time())}: {i}/{n}\n")
            try:
                repo.pull(headnames=chunk)
            except error.RepoLookupError as e:
                print(e)

    ui.write(f"generating merge data from file {filepath} ...\n")
    merge_commits = get_merge_commits_from_file(filepath)

    commits = list(dict.fromkeys([c for group in merge_commits for c in group]))
    ui.write(f"prefetching {len(commits)} commits ...\n")
    prefetch_commits(repo, commits)
    ui.write(f"prefetching done\n")

    ctxs = []
    nonlinear_merge = 0
    lookuperr = 0
    n = len(merge_commits)
    for i, (p1, p2, merge_commit) in enumerate(merge_commits, start=1):
        try:
            p2ctx = repo[p2]
            parents = repo.dageval(lambda: parentnames(p2ctx.node()))
            if len(parents) != 1:
                nonlinear_merge += 1
                continue
            basectx = repo[parents[0]]
            p1ctx = repo[p1]
            mergectx = repo[merge_commit]
            ctxs.append((p1ctx, p2ctx, basectx, mergectx))
        except error.RepoLookupError:
            lookuperr += 1
        if i % 100 == 0:
            ui.write(f"{int(time.time())}: {i}/{n} lookuperr={lookuperr}\n")

    ui.write(f"len(merge_ctxs)={len(ctxs)}, nonlinear_merge={nonlinear_merge}\n")
    return ctxs


def merge_file(repo, dstctx, srcctx, basectx, mergectx, filepath, algos, bench_stats):
    srctext = srcctx[filepath].data()
    dsttext = dstctx[filepath].data()
    basetext = basectx[filepath].data()

    if srctext == dsttext or srctext == basetext or dsttext == basetext:
        return

    bench_stats.changed_files += 1

    m3 = gen_3way_merger(repo.ui, basetext, dsttext, srctext, filepath, algos)
    mergedlines, conflictscount = render_minimized(m3)
    mergedtext = b"".join(mergedlines)

    if conflictscount:
        bench_stats.unresolved_files += 1
    else:
        expectedtext = mergectx[filepath].data()
        if remove_white_space(mergedtext) != remove_white_space(expectedtext):
            bench_stats.unmatched_files += 1
            mergedtext_baseline = b""

            if algos:
                m3_baseline = Merge3Text(basetext, dsttext, srctext)
                mergedtext_baseline = b"".join(render_minimized(m3_baseline)[0])

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


def readfile(path):
    with open(path, "rb") as f:
        return f.read()


def remove_white_space(text):
    return re.sub(WHITE_SPACE_PATTERN, b"", text)


def str_to_tuple(csv, sep=","):
    return tuple(csv.split(sep) if csv else ())


if __name__ == "__main__":
    import doctest

    doctest.testmod()
