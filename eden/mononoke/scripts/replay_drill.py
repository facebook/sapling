# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

"""Parallel commit-throughput drill for Mononoke.

Measures how fast commits can be landed to a dedicated bookmark (default
``mononoke_commit_throughput_test``: no hooks, non-fast-forward allowed, a
derivation-pipeline target alongside master) by landing many small stacks
CONCURRENTLY.

It replays the real content of recent master commits, partitioned into
FILE-DISJOINT stacks (each <= ``--max-stack-size``; a commit that would extend a
full stack, or span two stacks, is dropped) so concurrent pushrebases never
conflict. Every changed file is perturbed with a nonce that is a hash of its own
content, so blob/manifest reuse mirrors the original history while the perturbed
blobs are globally fresh -- forcing real derivation work as the warm bookmark
cache advances the bookmark.

Flow: build each stack as siblings off the bookmark's current tip -> upload all
heads once with ``sl cloud upload`` (EdenAPI, the same primitive ``jf submit``
uses) -> land them all at once via SCS ``repo_land_stack``. Measures aggregate
commits/sec, per-stack land latency, server pushrebase distance, and retry counts.

The file filter is the bot's own Phabricator ACL ``allowed_directories``
(``phabricator/service_user_configs/<fbid>``, read live via the configerator
consumption API): only files under those directories are replayed. The bot only
ever has directory-scoped access to fbsource, so the replay is always filtered.

Commits are authored by the SCM Commit Throughput Bot; source control derives the
service user from the FBID in the author e-mail to authorize the push-without-diff
land -- the same attribution the ``sl push`` path used. Validate a single land
first with ``--count 1`` before large runs.

Example:
    buck run fbcode//eden/mononoke/scripts:replay_drill -- \\
        --count 1000 --max-stack-size 5

Local smoke (build + group, no upload/land; use a scratch checkout):
    buck run fbcode//eden/mononoke/scripts:replay_drill -- --dry-run --count 20
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import logging
import os
import re
import statistics
import subprocess
import time
from typing import Any, TypedDict

from configerator.client import ConfigeratorClient
from scm.service.thrift.source_control import types as scs
from scm.service.thrift.source_control.clients import SourceControlService
from servicerouter.py3 import ClientParams, get_sr_client

logger: logging.Logger = logging.getLogger(__name__)


class LandResult(TypedDict):
    """Outcome of one SCS repo_land_stack call."""

    size: int
    latency: float
    submit: float  # seconds from batch start to when this land was submitted
    done: float  # seconds from batch start to when its response returned
    distance: int
    retry: int


BOT_FBID = 1704614544113542
# fbsource repo FBID -- the key into the bot's per-repo permission map.
FBSOURCE_REPO_FBID = 622826987896477
BOT_AUTHOR = "SCM Commit Throughput Bot <generatedunixname1704614544113542@meta.com>"
DEFAULT_BOOKMARK = "mononoke_commit_throughput_test"
NONCE_MARKER = "# drill-nonce"
# Server-side limit_filesize hook rejects any file over 5 MiB; materialize_commit
# drops files above this from the synthetic commit. Margin covers the added nonce.
MAX_FILE_SIZE = 5 * 1024 * 1024 - 4096
# In-flight cap for the read-only ACL pre-check (not a throughput knob).
ACL_CHECK_CONCURRENCY = 32
# Sharded SCS tier; the unsharded "mononoke-scs-server" is legacy. Requests must
# carry the ShardManager domain (the repo name) to route to the right shard.
SCS_TIER = "shardmanager:mononoke.scs"
SCS_CLIENT_ID = "mononoke-commit-throughput-drill"


def sl(
    args: list[str], *, check: bool = True, capture: bool = True
) -> subprocess.CompletedProcess[str]:
    """Run an ``sl`` command; on failure surface the captured stdout/stderr."""
    proc = subprocess.run(["sl", *args], check=False, text=True, capture_output=capture)
    if check and proc.returncode != 0:
        detail = ""
        if capture:
            detail = f"\n--- stdout ---\n{proc.stdout}\n--- stderr ---\n{proc.stderr}"
        raise SystemExit(
            f"`sl {' '.join(args)}` failed (exit {proc.returncode}){detail}"
        )
    return proc


def sl_out(args: list[str]) -> str:
    return sl(args).stdout.strip()


def content_nonce(data: bytes) -> bytes:
    """Append a content-derived nonce so the blob is fresh but deterministic.

    The nonce is a hash of ``data`` alone (not the path), so two files with
    identical content produce identical blobs -- reproducing the deduplication
    the original history had -- while never matching a real, already-derived
    blob.
    """
    digest = hashlib.sha256(data).hexdigest()
    return data + f"\n{NONCE_MARKER} {digest}\n".encode()


# --- ACL / file-filter detection ------------------------------------------


def read_allowed_directories(fbid: int) -> set[str]:
    """Load the bot's fbsource ``allowed_directories`` from its materialized ACL.

    Reads the deployed config via the configerator consumption API (not the
    ``.cconf`` source in a checkout), so it reflects what is actually live.
    """
    cfg = ConfigeratorClient().get_config_contents_as_JSON(
        f"phabricator/service_user_configs/{fbid}"
    )
    perms = cfg["permission_set"]["permissions"].get(str(FBSOURCE_REPO_FBID))
    if perms is None:
        raise SystemExit(f"Bot {fbid} has no fbsource permission in its ACL config.")
    return set(perms.get("allowed_directories") or [])


def bot_prefixes() -> list[str]:
    """The bot's fbsource allowed directories -- the drill's file filter.

    The bot only ever has directory-scoped access to fbsource (never the whole
    repo), so the replay is always restricted to these prefixes.
    """
    prefixes = sorted(read_allowed_directories(BOT_FBID))
    if not prefixes:
        raise SystemExit("Bot has no fbsource allowed_directories in its ACL config.")
    return prefixes


def assert_paths_allowed(paths: list[str], prefixes: list[str]) -> None:
    """Fail before committing if any path escapes the bot's allowed directories."""
    for path in paths:
        if not any(path.startswith(prefix) for prefix in prefixes):
            raise SystemExit(
                f"Refusing to commit '{path}': outside allowed directories {prefixes}"
            )


# --- Commit range + materialization ---------------------------------------


def commits_touching(prefixes: list[str], count: int) -> list[str]:
    """The ``count`` most-recent first-parent master commits touching ``prefixes``.

    Returned oldest-first. ``--follow --limit`` walks the mainline lazily and
    stops once ``count`` matching commits are found, so it does not scan all of
    history.
    """
    out = sl(
        ["log", "--follow", "--limit", str(count), "-r", "master", *prefixes]
        + ["-T", "{node}\n"]
    ).stdout
    return list(reversed([node for node in out.splitlines() if node]))


def _under(path: str, prefixes: list[str]) -> bool:
    return any(path.startswith(prefix) for prefix in prefixes)


def _changed_paths(node: str, prefixes: list[str]) -> tuple[list[str], list[str]]:
    """Return (written, removed) paths of a real commit, filtered to ``prefixes``."""
    present = sl(["status", "--change", node, "--no-status", "-a", "-m"]).stdout
    gone = sl(["status", "--change", node, "--no-status", "-r", "-d"]).stdout
    written = [p for p in present.splitlines() if p and _under(p, prefixes)]
    removed = [p for p in gone.splitlines() if p and _under(p, prefixes)]
    return written, removed


def changed_files_for(node: str, prefixes: list[str]) -> set[str]:
    """The set of files a real commit touches (filtered to ``prefixes``).

    ``materialize_commit`` recreates each file's end-state as a plain ``sl add``
    (no copy metadata), so the synthetic commit only ever touches these paths --
    that is what pushrebase compares, so grouping on them is exact. File-size is
    not checkable cheaply here (the ``size()`` fileset scans the whole tree);
    oversized files are dropped later in ``materialize_commit`` instead.
    """
    written, removed = _changed_paths(node, prefixes)
    return set(written) | set(removed)


def _revert_files(node: str, written: list[str]) -> list[str]:
    """Revert ``written`` to their ``node`` end-state; return those that succeeded.

    One bulk ``sl revert``, falling back to per-file so a single unreadable path
    (ACL-restricted or special) doesn't lose the whole commit.
    """
    if not sl(
        ["revert", "-r", node, "--no-backup", *written], capture=False, check=False
    ).returncode:
        return written
    return [
        p
        for p in written
        if not sl(
            ["revert", "-r", node, "--no-backup", p], capture=False, check=False
        ).returncode
    ]


def _nonce_in_place(root: str, path: str) -> bool:
    """Content-nonce one reverted file in place; return whether it was kept.

    Reverts back to base (dropping from the synthetic commit) anything that isn't
    a plain regular file -- symlinks (nonce-ing would corrupt the target),
    dangling symlinks, special files, or a "no such file in rev" no-op -- or that
    exceeds the server file-size limit. Such a path would otherwise abort
    ``sl add``.
    """
    dest = f"{root}/{path}"
    if os.path.islink(dest) or not os.path.isfile(dest):
        sl(["revert", "--no-backup", path], capture=False, check=False)
        return False
    with open(dest, "rb") as fh:
        data = fh.read()
    if len(data) > MAX_FILE_SIZE:
        sl(["revert", "--no-backup", path], capture=False, check=False)
        return False
    with open(dest, "wb") as fh:
        fh.write(content_nonce(data))
    return True


def materialize_commit(root: str, node: str, prefixes: list[str]) -> list[str] | None:
    """Recreate a real commit's end-state (content-nonced) on the current parent.

    Only files under the bot's allowed directories (``prefixes``) are replayed.
    Materializing the end-state -- not applying a diff -- keeps this conflict-free
    against an arbitrary base. Returns ``None`` (skip the stack) when nothing
    materializes or ``sl add`` fails, rather than aborting the run; the next
    ``goto --clean`` discards partial state. (Commits touching ACL paths are
    dropped up front in ``_restricted_nodes``, so this is only a backstop.)
    """
    written, removed = _changed_paths(node, prefixes)
    for path in removed:
        sl(["remove", "--force", path], check=False)
    if written:
        written = _revert_files(node, written)
        if not written:
            return None
        written = [p for p in written if _nonce_in_place(root, p)]
        if written and sl(["add", *written], check=False).returncode:
            return None
    return written + removed


# --- Bookmark -------------------------------------------------------------


def clean_base() -> str:
    """A clean, local commit to build/land on: the local master tip.

    Real master is already fully derived and carries no prior-run drill commits,
    so building here (and resetting the bookmark to it) keeps every land at
    pushrebase distance ~0 -- no rebase across, or collision with, replayed paths.
    """
    return sl_out(["log", "-r", "master", "-T", "{node}"])


# --- File-disjoint stack grouping -----------------------------------------


def group_disjoint_stacks(
    nodes: list[str], files_by_node: dict[str, set[str]], max_size: int
) -> tuple[list[list[str]], int, list[tuple[str, int]]]:
    """Greedily group commits into file-disjoint stacks (each <= ``max_size``).

    Invariant: every file is owned by at most one stack, so stacks landed
    concurrently never touch the same file (no pushrebase conflict). Each commit
    extends its owning stack if it shares files with one (and that stack has
    room), otherwise it starts a new stack -- the stack count is not capped. A
    commit is dropped when its files span two stacks, or would extend a stack
    already at ``max_size``. Returns (stacks oldest-first, dropped count, top
    drop-causing files).
    """
    owner: dict[str, int] = {}
    stacks: list[list[str]] = []
    dropped = 0
    drop_files: dict[str, int] = {}

    def note_drop(files: set[str]) -> None:
        nonlocal dropped
        dropped += 1
        for f in files:
            if f in owner:
                drop_files[f] = drop_files.get(f, 0) + 1

    for node in nodes:
        files = files_by_node[node]
        if not files:
            continue  # no replayable change (e.g. all outside allowed dirs)
        overlapping = {owner[f] for f in files if f in owner}
        if len(overlapping) > 1:
            note_drop(files)
            continue
        if len(overlapping) == 1:
            s = next(iter(overlapping))
            if max_size and len(stacks[s]) >= max_size:
                note_drop(files)
                continue
        else:
            s = len(stacks)
            stacks.append([])
        stacks[s].append(node)
        for f in files:
            owner[f] = s

    hot = sorted(drop_files.items(), key=lambda kv: kv[1], reverse=True)[:5]
    return stacks, dropped, hot


def build_sibling_stacks(
    args: argparse.Namespace,
    prefixes: list[str],
    stacks: list[list[str]],
    base: str,
) -> list[tuple[str, int]]:
    """Build each stack as a sibling off ``base``; return (head, commit count)."""
    root = sl_out(["root"])
    built: list[tuple[str, int]] = []
    skipped = 0
    for i, stack in enumerate(stacks):
        sl(["goto", "--clean", base], capture=False)
        tip = ""
        n = 0
        for node in stack:
            touched = materialize_commit(root, node, prefixes)
            if touched is None:
                # A file couldn't be materialized; abandon this stack. The next
                # goto --clean base discards the partial working-copy state.
                skipped += 1
                tip = ""
                break
            assert_paths_allowed(touched, prefixes)
            title = sl_out(["log", "-r", node, "-T", "{desc|firstline}"])
            if sl(["commit", "-u", args.author, "-m", title], check=False).returncode:
                # Commit failed (usually a no-op node). Discard any leftover
                # materialized changes so the next node doesn't inherit them and
                # break the file-disjointness invariant.
                sl(["goto", "--clean", "."], capture=False, check=False)
                continue
            tip = sl_out(["log", "-r", ".", "-T", "{node}"])
            n += 1
        if tip:
            built.append((tip, n))
        if (i + 1) % 50 == 0:
            logger.info("  built %d/%d stacks", i + 1, len(stacks))
    if skipped:
        logger.info("  skipped %d stacks that could not be materialized", skipped)
    return built


def upload_heads(heads: list[str]) -> None:
    """Upload all draft stack heads to the server in one EdenAPI upload.

    Same primitive ``jf submit`` uses; writes bonsai + bonsai_hg_mapping so SCS
    ``repo_land_stack`` can resolve the heads by hg id.
    """
    sl(["cloud", "upload", "-r", "+".join(heads)], capture=False)


# --- Parallel land via SCS ------------------------------------------------


def _scs_client_params(repo: str) -> ClientParams:
    """ClientParams for the sharded SCS tier: identifies the drill and sets the
    ShardManager domain (the repo name, with characters SM can't take encoded)
    so requests route to the repo's shard."""
    domain = repo.replace("/", "_SLASH_").replace("+", "_PLUS_")
    return ClientParams().setClientId(SCS_CLIENT_ID).setShardManagerDomain(domain)


async def _land_stack(
    client: Any, repo: Any, params: Any, size: int, t0: float
) -> LandResult:
    submit = time.monotonic() - t0
    resp = await client.repo_land_stack(repo, params)
    done = time.monotonic() - t0
    outcome = resp.pushrebase_outcome
    return {
        "size": size,
        "latency": done - submit,
        "submit": submit,
        "done": done,
        "distance": int(outcome.pushrebase_distance),
        "retry": int(outcome.retry_num),
    }


async def _reset_bookmark(args: argparse.Namespace, target_hex: str) -> None:
    """Reset the target bookmark to ``target_hex`` (a clean, local base).

    A backward/non-fast-forward move that discards the drill commits a prior run
    piled on, so this run builds and lands at pushrebase distance ~0 instead of
    rebasing across everything since -- which collides on re-replayed paths. The
    caller identity must be allowed to move the bookmark (the same identity that
    already moves it via ``repo_land_stack``); no service_identity is passed.
    """
    async with get_sr_client(
        SourceControlService, SCS_TIER, params=_scs_client_params(args.repo)
    ) as client:
        await client.repo_move_bookmark(
            scs.RepoSpecifier(name=args.repo),
            scs.RepoMoveBookmarkParams(
                bookmark=args.bookmark,
                target=scs.CommitId(hg=bytes.fromhex(target_hex)),
                allow_non_fast_forward_move=True,
            ),
        )


async def _restricted_nodes(args: argparse.Namespace, nodes: list[str]) -> set[str]:
    """Commits whose changed paths touch any ACL restriction (per SCS, the authority).

    ``commit_restricted_paths_changes`` extracts each commit's changed paths
    server-side and reports whether any is under a restriction root -- the
    complete set (every root, not just the first) and independent of whether we
    can read them. Dropping these up front means we never build a stack that
    ``materialize_commit`` can't reproduce. Runs concurrently, one call/commit; a
    failed check is treated as unrestricted (the build-time skip is the backstop).
    """
    repo = scs.RepoSpecifier(name=args.repo)
    params = scs.CommitRestrictedPathsChangesParams()
    sem = asyncio.Semaphore(ACL_CHECK_CONCURRENCY)

    async def check(client: Any, node: str) -> tuple[str, bool]:
        async with sem:
            commit = scs.CommitSpecifier(
                repo=repo, id=scs.CommitId(hg=bytes.fromhex(node))
            )
            resp = await client.commit_restricted_paths_changes(commit, params)
        return node, resp.are_restricted != scs.PathCoverage.NONE

    async with get_sr_client(
        SourceControlService, SCS_TIER, params=_scs_client_params(args.repo)
    ) as client:
        results = await asyncio.gather(
            *(check(client, n) for n in nodes), return_exceptions=True
        )
    errors = [r for r in results if isinstance(r, BaseException)]
    if errors:
        logger.warning(
            "%d/%d ACL pre-checks failed (treated as unrestricted); first: %r",
            len(errors),
            len(nodes),
            errors[0],
        )
    return {r[0] for r in results if isinstance(r, tuple) and r[1]}


async def _land_all(
    args: argparse.Namespace, heads: list[tuple[str, int]], base_hex: str
) -> list[Any]:
    repo = scs.RepoSpecifier(name=args.repo)
    base = scs.CommitId(hg=bytes.fromhex(base_hex))
    schemes = {scs.CommitIdentityScheme.HG}
    # Fire every stack at once (asyncio, so this parks no threads -- each land
    # just awaits its server response).
    async with get_sr_client(
        SourceControlService, SCS_TIER, params=_scs_client_params(args.repo)
    ) as client:
        t0 = time.monotonic()
        tasks = [
            _land_stack(
                client,
                repo,
                scs.RepoLandStackParams(
                    bookmark=args.bookmark,
                    head=scs.CommitId(hg=bytes.fromhex(head_hex)),
                    base=base,
                    identity_schemes=schemes,
                ),
                size,
                t0,
            )
            for head_hex, size in heads
        ]
        return await asyncio.gather(*tasks, return_exceptions=True)


def _report(results: list[Any], wall: float, num_stacks: int, dropped: int) -> None:
    ok = [r for r in results if isinstance(r, dict)]
    landed = sum(int(r["size"]) for r in ok)
    rate = landed / wall if wall else 0.0
    logger.info(
        "\nParallel done: %d/%d stacks landed, %d commits over %.1fs wall "
        "(%.2f commits/sec); %d dropped.",
        len(ok),
        num_stacks,
        landed,
        wall,
        rate,
        dropped,
    )
    for r in results:
        if not isinstance(r, dict):
            logger.warning("  stack land failed: %r", r)
    submits = [float(r["submit"]) for r in ok]
    dones = [float(r["done"]) for r in ok]
    if submits:
        logger.info(
            "Submit window: all %d lands submitted between %.3fs and %.3fs "
            "(spread %.3fs); responses returned between %.3fs and %.3fs.",
            len(ok),
            min(submits),
            max(submits),
            max(submits) - min(submits),
            min(dones),
            max(dones),
        )
    latencies = sorted(float(r["latency"]) for r in ok)
    if latencies:
        p90 = (
            statistics.quantiles(latencies, n=10)[-1]
            if len(latencies) > 1
            else latencies[0]
        )
        logger.info(
            "Per-stack land latency: avg %.3fs, p50 %.3fs, p90 %.3fs, max %.3fs.",
            statistics.fmean(latencies),
            statistics.median(latencies),
            p90,
            max(latencies),
        )
    distances = [int(r["distance"]) for r in ok]
    if distances:
        logger.info(
            "Server pushrebase distance: p50 %d, max %d.",
            int(statistics.median(distances)),
            max(distances),
        )
    retries = [int(r["retry"]) for r in ok]
    if retries:
        logger.info(
            "Retry count: p50 %d, max %d, %d stacks retried.",
            int(statistics.median(retries)),
            max(retries),
            sum(1 for x in retries if x > 0),
        )


def run_drill(args: argparse.Namespace, prefixes: list[str]) -> None:
    nodes = commits_touching(prefixes, args.count)
    if not nodes:
        raise SystemExit("No master commits found to replay.")

    # Drop commits touching any ACL-restricted path (per SCS, which is complete
    # and authoritative here, regardless of our own access), so we never build a
    # stack that materialize_commit can't reproduce.
    restricted = asyncio.run(_restricted_nodes(args, nodes))
    if restricted:
        logger.info(
            "Excluding %d/%d commits touching ACL-restricted paths.",
            len(restricted),
            len(nodes),
        )
        nodes = [n for n in nodes if n not in restricted]
    if not nodes:
        raise SystemExit("Every candidate commit touches ACL-restricted paths.")

    files_by_node = {n: changed_files_for(n, prefixes) for n in nodes}
    base = clean_base()  # every stack builds/lands on the local master tip

    stacks, dropped, hot = group_disjoint_stacks(
        nodes, files_by_node, args.max_stack_size
    )
    sizes = sorted(len(s) for s in stacks)
    logger.info(
        "%d commits -> %d disjoint stacks "
        "(size min %d / median %d / max %d), %d dropped.",
        len(nodes),
        len(stacks),
        sizes[0] if sizes else 0,
        int(statistics.median(sizes)) if sizes else 0,
        sizes[-1] if sizes else 0,
        dropped,
    )
    if hot:
        logger.info(
            "  hot files capping parallelism: %s",
            ", ".join(f"{f} (x{c})" for f, c in hot),
        )
    if not stacks:
        logger.info("Nothing to push.")
        return

    if args.dry_run:
        anchor = sl_out(["log", "-r", f"{nodes[0]}~1", "-T", "{node}"])
        built = build_sibling_stacks(args, prefixes, stacks, anchor)
        logger.info("[dry-run] built %d stacks; not uploading or landing.", len(built))
        return

    # Reset the bookmark to the clean base (master tip), then build and land every
    # stack on it, so each stack's pushrebase root is that base -- distance ~0,
    # never rebasing across (or colliding with) commits a prior run piled on.
    logger.info("Resetting %s to clean base %s...", args.bookmark, base[:12])
    asyncio.run(_reset_bookmark(args, base))
    heads = build_sibling_stacks(args, prefixes, stacks, base)
    if not heads:
        logger.info("Nothing built; nothing to land.")
        return
    upload_heads([h for h, _ in heads])
    logger.info(
        "Landing %d stacks on base %s (all at once)...",
        len(heads),
        base[:12],
    )
    wall_start = time.monotonic()
    results = asyncio.run(_land_all(args, heads, base))
    _report(results, time.monotonic() - wall_start, len(stacks), dropped)


# --- CLI ------------------------------------------------------------------


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--count", type=int, default=1000, help="Number of recent commits to replay."
    )
    parser.add_argument("--bookmark", default=DEFAULT_BOOKMARK, help="Target bookmark.")
    parser.add_argument("--author", default=BOT_AUTHOR, help="Commit author.")
    parser.add_argument(
        "--max-stack-size",
        type=int,
        default=5,
        help="Max commits per stack (0 = unlimited).",
    )
    parser.add_argument("--repo", default="fbsource", help="Repo name for SCS calls.")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Build + group locally but do not upload or land (local smoke test).",
    )
    parser.add_argument(
        "--hide-commits",
        action="store_true",
        help="After the run, hide the drill-generated commits (default: keep them).",
    )
    return parser.parse_args()


def hide_generated(author: str) -> None:
    """Hide the drill's leftover draft commits (those authored by the bot)."""
    match = re.search(r"<([^>]+)>", author)
    needle = match.group(1) if match else author
    # Escape for embedding in a single-quoted revset string literal.
    escaped = needle.replace("\\", "\\\\").replace("'", "\\'")
    sl(["hide", "-r", f"draft() & author('{escaped}')"], check=False)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(message)s")
    args = parse_args()
    if sl_out(["status"]):
        raise SystemExit("Working copy has pending changes; commit or shelve them.")

    start = sl_out(["log", "-r", ".", "-T", "{node}"])
    prefixes = bot_prefixes()
    logger.info(
        "Replaying under %d allowed dir(s); target bookmark %s.",
        len(prefixes),
        args.bookmark,
    )

    try:
        run_drill(args, prefixes)
    finally:
        sl(["goto", "--clean", start], capture=False)
        if args.hide_commits:
            hide_generated(args.author)


if __name__ == "__main__":
    main()
