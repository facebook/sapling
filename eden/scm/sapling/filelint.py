# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Lint and fix file content from commit contexts without checking it out."""

import concurrent.futures
import contextlib
import os
import shutil
import subprocess
import tempfile

import bindings

from . import context, error, match as matchmod, util
from .i18n import _, _n


_DEFAULT_MAX_FILE_COUNT = 100_000
_DEFAULT_LINTER_JOBS = 8
_DEFAULT_MAX_FILES_PER_COMMAND = 2_500
_CONFIG_FILE_PREFIX = "config-file."
_MAX_SHOWN_ISSUES = 5


class _Linter:
    """A configured `filelint.linter.<name>.*` tool."""

    def __init__(self, name, command, configfiles, stagingsymlinks):
        self.name = name
        self.command = command
        self.configfiles = configfiles
        self.stagingsymlinks = stagingsymlinks


class _LintWarnings:
    def __init__(self, max_file_size, max_file_count):
        self.max_file_size = max_file_size
        self.max_file_count = max_file_count
        self.counts = {}

    def add(self, reason, count=1):
        if count > 0:
            self.counts[reason] = self.counts.get(reason, 0) + count

    def emit(self, ui):
        for reason, count in self.counts.items():
            if reason == "config-file":
                ui.warn(
                    _("warning: can't lint %d linter configuration file(s)\n") % count
                )
            elif reason == "file-count":
                ui.warn(
                    _(
                        "warning: can't lint %d file(s) because filelint.max-file-count is %d\n"
                    )
                    % (count, self.max_file_count)
                )
            elif reason == "fixed-size":
                ui.warn(
                    _(
                        "warning: can't lint %d file(s) whose fixed content exceeds %d bytes\n"
                    )
                    % (count, self.max_file_size)
                )
            elif reason == "oversized":
                ui.warn(
                    _("warning: can't lint %d file(s) larger than %d bytes\n")
                    % (count, self.max_file_size)
                )
            elif reason == "staging-link":
                ui.warn(
                    _(
                        "warning: can't lint %d file(s) under linter staging-symlinks paths\n"
                    )
                    % count
                )


@contextlib.contextmanager
def lintctxs(repo, ctxs):
    """Yield fixed output paths for each context.

    The yielded paths point into a staging tree that is removed when the
    context manager exits, so callers must consume them (ex. replay via
    `overlayctx` and `commitctx`) before exiting.
    """
    # Always present via the builtin core config.
    max_file_size = repo.ui.configbytes("filelint", "max-file-size")
    max_file_count = repo.ui.configint(
        "filelint", "max-file-count", _DEFAULT_MAX_FILE_COUNT
    )
    linters = _linters(repo.ui)
    warnings = _LintWarnings(max_file_size, max_file_count)
    config_file_names = _configfilenames(linters)
    links = _linknames(linters)
    candidates = _findcandidates(repo, ctxs, config_file_names, links, warnings)
    replacements = [{} for _ctx in ctxs]
    if not candidates:
        warnings.emit(repo.ui)
        yield replacements
        return
    if not linters:
        raise error.Abort(_("no filelint linters are configured"))

    state = bindings.filelint.lintstate(repo._rsrepo)
    with _stagingroot(repo) as output_root:
        _runlinters(
            repo,
            state,
            linters,
            # Discover linter configuration files from the last (highest)
            # selected context, so results do not depend on where the working
            # copy happens to be.
            ctxs[-1].manifest(),
            candidates,
            replacements,
            output_root,
            links,
            max_file_count,
            config_file_names,
            warnings,
        )
        warnings.emit(repo.ui)
        yield replacements


def overlayctx(ctx, parents, replacements, operation):
    """Replay a context onto new parents with selected file content replaced."""
    mctx = context.memctx.mirrorformutation(ctx, operation, parents=parents)
    for path, fixed_path in replacements.items():
        mctx[path] = context.overlayfilectx(
            ctx[path],
            datafunc=lambda fixed_path=fixed_path: _readfile(fixed_path),
            ctx=mctx,
        )
    return mctx


def _linters(ui):
    """Load the configured `filelint.linter.<name>.*` tools.

    Only fixing linters using the staging-tree mode are supported so far;
    validate-only linters and other modes (ex. stdin) are skipped.
    """
    names = set()
    for key, _value in ui.configitems("filelint"):
        if key.startswith("linter."):
            names.add(key.split(".")[1])
    linters = []
    for name in sorted(names):
        prefix = "linter.%s." % name
        if not ui.configbool("filelint", prefix + "fix"):
            continue
        if ui.config("filelint", prefix + "mode") != "staging-tree":
            continue
        command = ui.configlist("filelint", prefix + "command")
        if not command:
            continue
        configfiles = set()
        for key, _value in ui.configitems("filelint"):
            if key.startswith(prefix + _CONFIG_FILE_PREFIX):
                configfiles.update(ui.configlist("filelint", key))
        stagingsymlinks = ui.configlist("filelint", prefix + "staging-symlinks")
        linters.append(_Linter(name, command, configfiles, stagingsymlinks))
    return linters


def _configfilenames(linters):
    """Combine every linter's configuration filename lists."""
    names = set()
    for linter in linters:
        names.update(linter.configfiles)
    return names


def _linknames(linters):
    """Combine every linter's staging symlink lists."""
    names = set()
    for linter in linters:
        names.update(linter.stagingsymlinks)
    return sorted(names)


def _findcandidates(repo, ctxs, config_file_names, links, warnings):
    """Collect each context's changed file versions."""
    candidates = []
    for ctxindex, ctx in enumerate(ctxs):
        if len(ctx.parents()) > 1:
            continue
        manifest = ctx.manifest()
        for path in sorted(set(ctx.files())):
            if path not in manifest:
                # Removed by this commit.
                continue
            node, flags = manifest.find(path)
            if flags in ("l", "m"):
                continue
            if _skipconfigfile(path, config_file_names, warnings):
                continue
            if _skipstaginglink(path, links, warnings):
                continue
            candidates.append((ctxindex, path, node, flags))
    return candidates


def _partitioncandidates(candidates, casesensitive):
    """Pack distinct versions into file trees with one version of each path.

    Trees are keyed by the staging filesystem's view of the path: on a case
    insensitive filesystem, paths differing only by case go into separate
    trees so they never alias one on-disk file.
    """
    filetrees = []
    targets = {}
    assignments = {}
    pathversions = {}
    for ctxindex, path, node, flags in candidates:
        key = (path, node)
        filetreeindex = assignments.get(key)
        if filetreeindex is None:
            foldedpath = util.normcase(path, casesensitive=casesensitive)
            filetreeindex = pathversions.get(foldedpath, 0)
            pathversions[foldedpath] = filetreeindex + 1
            while len(filetrees) <= filetreeindex:
                filetrees.append([])
            filetrees[filetreeindex].append((path, node, flags))
            assignments[key] = filetreeindex
        targets.setdefault((filetreeindex, path), []).append(ctxindex)
    return filetrees, targets


def _skipconfigfile(path, config_file_names, warnings):
    if os.path.basename(path) not in config_file_names:
        return False
    warnings.add("config-file")
    return True


def _skipstaginglink(path, links, warnings):
    """Skip files that a staging link would shadow.

    Linked entries resolve into the working copy, so staging a candidate
    under one would write outside the staging tree.
    """
    if not any(path == link or path.startswith(link + "/") for link in links):
        return False
    warnings.add("staging-link")
    return True


def _findconfigfiles(repo, manifest, candidates, config_file_names, casesensitive):
    """Find configured filenames in candidate ancestor directories."""
    if not config_file_names:
        return []
    # The repository root is every candidate's ancestor, so it is always
    # searched even though the parent walk below never produces it.
    directories = {""}
    for _ctxindex, path, _node, _flags in candidates:
        parent = path.rpartition("/")[0]
        while parent:
            directories.add(parent)
            parent = parent.rpartition("/")[0]
    # Match files directly inside each directory, narrowed to the configured
    # basenames. The walk visits only the listed directories, so sibling
    # trees (possibly ACL restricted) are never fetched.
    matcher = matchmod.intersectmatchers(
        matchmod.match(
            repo.root,
            "",
            ["rootfilesin:%s" % directory for directory in directories],
        ),
        # Match names with the staging filesystem's case sensitivity, since
        # that decides which staged files the linters' own configuration
        # lookups can see.
        matchmod.basenamematcher(
            repo.root, "", config_file_names, casesensitive=casesensitive
        ),
    )
    configs = []
    for path in manifest.walk(matcher):
        node, flags = manifest.find(path)
        configs.append((path, node, flags))
    return sorted(configs)


def _prefiltercandidates(repo, state, candidates, warnings):
    """Drop oversized and already lint-clean versions before fetching content."""
    versions = list(
        dict.fromkeys((path, node) for _ctxindex, path, node, _flags in candidates)
    )
    result = state.prefilter(versions)
    warnings.add("oversized", result["oversized_files"])
    repo.ui.log(
        "filelint",
        filelint_candidate_versions=len(versions),
        filelint_clean_versions=result["clean_files"],
        filelint_oversized_versions=result["oversized_files"],
    )
    keep = set(result["files"])
    return [
        candidate for candidate in candidates if (candidate[1], candidate[2]) in keep
    ]


def _makefilewrites(filetrees, targets, configs):
    """Expand file trees into explicit native write requests."""
    writes = []
    versions = {}
    for index, files in enumerate(filetrees):
        for path, node, flags in files:
            destination = "%d/%s" % (index, path)
            versions[destination] = (path, targets[(index, path)])
            writes.append((path, node, flags, destination))
        writes.extend(
            (path, node, flags, "%d/%s" % (index, path))
            for path, node, flags in configs
        )
    return writes, versions


def _runlinters(
    repo,
    state,
    linters,
    configmanifest,
    candidates,
    replacements,
    output_root,
    links,
    max_file_count,
    config_file_names,
    warnings,
):
    """Materialize versions, run linters over them, and map fixed outputs."""
    casesensitive = bindings.io.vfs(output_root).case_sensitive()
    configs = _findconfigfiles(
        repo, configmanifest, candidates, config_file_names, casesensitive
    )
    candidates = _prefiltercandidates(repo, state, candidates, warnings)
    if len(candidates) > max_file_count:
        warnings.add("file-count", len(candidates) - max_file_count)
        candidates = candidates[:max_file_count]
    if not candidates:
        return
    filetrees, targets = _partitioncandidates(candidates, casesensitive)
    writes, versions = _makefilewrites(filetrees, targets, configs)
    written = state.materialize(output_root, writes)
    fingerprints = {
        destination: (size, blake3)
        for destination, size, blake3 in written
        if destination in versions
    }
    if not fingerprints:
        return
    _stagelinks(repo, output_root, len(filetrees), links)
    repo.ui.status(
        _("running linters: %s\n") % ", ".join(linter.name for linter in linters)
    )
    current = dict(fingerprints)
    oversized = set()
    for index, linter in enumerate(linters):
        _lintfiletrees(repo, linter, output_root, current)
        # Only the last comparison records content as lint clean: earlier
        # outputs are not fixed points of the whole linter sequence.
        record = index == len(linters) - 1
        result = _comparestaged(state, output_root, versions, current, record)
        if record:
            oversized = set(result["oversized_files"])
            warnings.add("fixed-size", len(oversized))
        issuecounts = {}
        for destination, size, blake3 in result["changed_files"]:
            path, ctxindices = versions[destination]
            issuecounts[path] = issuecounts.get(path, 0) + len(ctxindices)
            current[destination] = (size, blake3)
        _showissues(repo.ui, linter.name, issuecounts)
    for destination, fingerprint in current.items():
        if destination in oversized or fingerprint == fingerprints[destination]:
            continue
        path, ctxindices = versions[destination]
        fixed_path = os.path.join(output_root, destination)
        for ctxindex in ctxindices:
            replacements[ctxindex][path] = fixed_path


def _lintfiletrees(repo, linter, output_root, fingerprints):
    jobs = repo.ui.configint("filelint", "jobs", _DEFAULT_LINTER_JOBS)
    batch_size = repo.ui.configint(
        "filelint", "max-files-per-command", _DEFAULT_MAX_FILES_PER_COMMAND
    )
    if min(jobs, batch_size) <= 0:
        raise error.Abort(_("filelint concurrency limits must be positive"))
    # Each tree is its own linter project root (see `_stagelinks`), so the
    # linter runs from the tree directory with tree-relative paths, which
    # match its path rules exactly like repository-relative paths do.
    bytree = {}
    for destination in sorted(fingerprints):
        treeindex, _sep, path = destination.partition("/")
        bytree.setdefault(treeindex, []).append(path)
    work = []
    for treeindex, paths in sorted(bytree.items()):
        treedir = os.path.join(output_root, treeindex)
        work.extend(
            (treedir, paths[start : start + batch_size])
            for start in range(0, len(paths), batch_size)
        )
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as executor:
        futures = [
            executor.submit(
                _runlinterprocess, linter.name, linter.command, treedir, batch
            )
            for treedir, batch in work
        ]
        try:
            for future in futures:
                future.result()
        except Exception:
            # Started batches run to completion, but don't launch queued
            # ones once one has failed.
            for future in futures:
                future.cancel()
            raise


def _comparestaged(state, output_root, versions, fingerprints, record):
    """Compare staged outputs against fingerprints, returning changed files."""
    requests = [
        (versions[destination][0], destination, size, fingerprint)
        for destination, (size, fingerprint) in fingerprints.items()
    ]
    return state.compare(output_root, requests, record)


def _showissues(ui, name, issuecounts):
    """Report one linter's issue count and a short list of affected files."""
    if not issuecounts:
        ui.note(_('Found 0 "%s" issues\n') % name)
        return
    total = sum(issuecounts.values())
    ui.status(
        _n('Found %d "%s" issue:\n', 'Found %d "%s" issues:\n', total) % (total, name)
    )
    entries = [
        path if count == 1 else _("%s (x%d)") % (path, count)
        for path, count in sorted(issuecounts.items())
    ]
    for entry in entries[:_MAX_SHOWN_ISSUES]:
        ui.status("  %s\n" % entry)
    if len(entries) > _MAX_SHOWN_ISSUES:
        ui.status(_("  ... and %d more\n") % (len(entries) - _MAX_SHOWN_ISSUES))


def lintworkingcopy(repo, paths):
    """Run every configured linter on working copy paths in place.

    Fixes land directly in files the user can inspect, so the linters' own
    output is forwarded instead of computing a separate report.
    """
    if not paths:
        return
    linters = _linters(repo.ui)
    if not linters:
        raise error.Abort(_("no filelint linters are configured"))
    paths = sorted(paths)
    for linter in linters:
        _runlinterprocess(linter.name, linter.command, repo.root, paths, ui=repo.ui)


@contextlib.contextmanager
def _stagingroot(repo):
    """Stage temporary file trees outside the repository.

    Each tree becomes its own linter project root (see `_stagelinks`), so
    trees must not live inside the repository, where linters would resolve
    paths against the repository root instead. A temporary directory also
    keeps staged writes on a regular filesystem even for virtualized
    working copies.
    """
    if repo.currentwlock() is None:
        raise error.ProgrammingError("file linting requires the working-copy lock")
    # `dir` re-reads TMPDIR on every call, unlike the cached
    # `tempfile.gettempdir` default.
    output_root = tempfile.mkdtemp(prefix="sapling-lint-", dir=os.environ.get("TMPDIR"))
    try:
        yield output_root
    finally:
        # Best effort: a cleanup failure must not mask the real error.
        shutil.rmtree(output_root, ignore_errors=True)


def _stagelinks(repo, output_root, treecount, links):
    """Link working-copy entries (ex. `tools`) into every staged tree.

    The links make each tree a self-contained linter project root: with
    `.arcconfig` present the linter treats the tree as its project root,
    so staged paths match its path rules without any prefix, while linter
    tooling and engine configuration resolve through the links into the
    working copy, just as they do when linting the working copy itself.
    """
    for index in range(treecount):
        treedir = os.path.join(output_root, "%d" % index)
        for name in links:
            source = os.path.join(repo.root, name.replace("/", os.sep))
            target = os.path.join(treedir, name.replace("/", os.sep))
            if not os.path.lexists(source):
                continue
            if os.path.lexists(target):
                # Already staged from the commit (ex. a config file that is
                # also listed as a link); the staged version wins.
                continue
            util.makedirs(os.path.dirname(target))
            _makelink(source, target)


def _makelink(source, target):
    if util.iswindows:
        # Symlinks need privileges on Windows: junctions cover directories
        # and the remaining linked entries are small files worth copying.
        if os.path.isdir(source):
            import _winapi

            _winapi.CreateJunction(source, target)
        else:
            shutil.copyfile(source, target)
        return
    os.symlink(source, target)


def _runlinterprocess(name, command, cwd, paths, ui=None):
    """Run one linter process from `cwd` with root-relative paths on stdin.

    `cwd` is the linter's project root: the repository root for working
    copy runs, or a staged tree directory. With `ui`, the linter's
    combined output is forwarded to the user's stderr (unless quiet)
    rather than captured silently.
    """
    forward = ui is not None and not ui.quiet
    input_data = b"\n".join(path.encode("utf-8") for path in paths) + b"\n"
    try:
        result = subprocess.run(
            list(command),
            cwd=cwd,
            input=input_data,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT if forward else subprocess.PIPE,
            close_fds=util.closefds,
        )
    except OSError as exc:
        raise error.Abort(
            _("failed to start linter %s '%s': %s") % (name, " ".join(command), exc)
        ) from exc
    if forward and result.stdout:
        ui.write_err(result.stdout.decode("utf-8", errors="replace"))
    if result.returncode:
        if forward:
            # The linter's output was already forwarded above.
            detail = _("exit status %d") % result.returncode
        else:
            detail = result.stderr or result.stdout
            detail = detail.decode("utf-8", errors="replace").strip()
            if not detail:
                detail = _("exit status %d") % result.returncode
        raise error.Abort(_("linter %s failed: %s") % (name, detail))


def _readfile(path):
    with open(path, "rb") as file:
        return file.read()
