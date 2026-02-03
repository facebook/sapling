# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""manage git working trees for parallel development

This extension provides commands for managing git worktrees when using
Sapling in Git mode. Worktrees allow you to check out multiple commits
simultaneously in separate directories, enabling parallel development
workflows such as running multiple AI coding agents.

Commands:

    sl wt add [NAME] [COMMIT]  - Create a new worktree
    sl wt list                 - List all worktrees
    sl wt remove PATH          - Remove a worktree

Configuration:

    [worktree]
    # Files to copy to new worktrees (supports patterns)
    copyfiles = .env
    copyfiles = .envrc
    copyfiles = .env.local
    copyfiles = .tool-versions
    copyfiles = mise.toml
    copyfiles = .mise.toml
    copyfiles = .claude/settings.local.json

    # Directories to copy with copy-on-write
    copydirs = node_modules

Example usage::

    # Create a worktree for the current commit
    sl wt add

    # Create a worktree with a custom name
    sl wt add feature-review

    # Create a worktree for a specific commit
    sl wt add review abc1234

    # List all worktrees
    sl wt list

    # Remove a worktree
    sl wt remove ../my-repo-feature-review
"""

import json
import os
import platform
import shutil
import subprocess

from sapling import error, registrar
from sapling.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)

# Default files to copy (configurable via sl config worktree.copyfiles)
DEFAULT_COPY_PATTERNS = [
    ".env",
    ".envrc",
    ".env.local",
    ".tool-versions",
    "mise.toml",
    ".mise.toml",
    ".claude/settings.local.json",
]

# Default directories to copy with CoW (configurable via sl config worktree.copydirs)
DEFAULT_COPY_DIRS = ["node_modules"]


@command(
    "wt",
    [],
    _("<add|list|remove>"),
    norepo=False,
)
def worktree_command(ui, repo, *args, **opts):
    """manage git working trees for parallel development

    Worktrees allow you to check out multiple commits simultaneously in
    separate directories. This is useful for:

    - Running multiple AI coding agents in parallel
    - Reviewing PRs without disturbing your current work
    - Testing changes in isolation

    Subcommands:

        add     Create a new working tree
        list    List all working trees
        remove  Remove a working tree

    Use 'sl wt SUBCOMMAND --help' for more information on a subcommand.
    """
    raise error.Abort(
        _(
            "you need to specify a subcommand (run with --help to see a list of subcommands)"
        )
    )


subcmd = worktree_command.subcommand(
    categories=[
        ("Create working trees", ["add"]),
        ("Manage working trees", ["list", "remove"]),
    ]
)


@subcmd(
    "add",
    [
        ("", "no-copy", False, _("skip copying untracked files")),
    ],
    _("[NAME] [COMMIT]"),
)
def add_cmd(ui, repo, name=None, commit=None, **opts):
    """create a new working tree for a commit

    Creates a git worktree as a sibling directory and optionally copies
    untracked config files using copy-on-write when possible.

    NAME defaults to the short commit hash (e.g., 'abc1234').
    COMMIT defaults to current working copy parent.

    Worktrees are created as siblings (../<repo>-<name>) to avoid appearing
    as untracked files in the main repo.

    Configure files to copy::

        sl config --local worktree.copyfiles ".env"
        sl config --local worktree.copyfiles ".npmrc"
        sl config --local worktree.copydirs "node_modules"

    Examples::

        # Create worktree for current commit (name = short hash)
        sl wt add

        # Create worktree with custom name
        sl wt add feature-x

        # Create worktree for specific commit
        sl wt add review abc1234

        # Skip copying untracked files
        sl wt add --no-copy
    """
    # Ensure we're in a git repo
    if not _is_git_repo(repo):
        raise error.Abort(_("worktree command requires a git repository"))

    # Resolve commit through Sapling (ensures it's in DAG)
    if commit is None:
        ctx = repo["."]
    else:
        try:
            ctx = repo[commit]
        except error.RepoLookupError:
            raise error.Abort(_("unknown revision '%s'") % commit)

    commit_hex = ctx.hex()
    commit_short = commit_hex[:8]

    # Generate name if not provided
    if name is None:
        name = commit_short

    # Build path as SIBLING to avoid untracked files issue
    # Always use ../repo-name-<wt-name> format (e.g., marketplace-abc1234)
    repo_basename = os.path.basename(repo.root)
    wt_dirname = f"{repo_basename}-{name}"
    wt_path = os.path.join(os.path.dirname(repo.root), wt_dirname)

    if os.path.exists(wt_path):
        raise error.Abort(_("path already exists: %s") % wt_path)

    # Call git worktree add --detach
    ui.status(_("creating worktree at %s...\n") % wt_path)
    result = subprocess.run(
        ["git", "worktree", "add", wt_path, commit_hex, "--detach"],
        cwd=repo.root,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise error.Abort(_("git worktree add failed: %s") % result.stderr.strip())

    # Copy untracked files (CoW when possible)
    if not opts.get("no_copy"):
        _copy_untracked_files(ui, repo, wt_path)

    ui.status(_("created worktree at %s\n") % wt_path)
    ui.status(_("  cd %s\n") % wt_path)

    return 0


@subcmd(
    "list",
    [
        ("", "json", False, _("output in JSON format")),
    ],
    "",
)
def list_cmd(ui, repo, **opts):
    """list all working trees

    Shows all git worktrees associated with this repository.

    Use --json to output machine-readable JSON format with the following fields:
    - path: absolute path to the worktree
    - commit: commit hash checked out in the worktree
    - branch: branch name (if any)
    - isMain: whether this is the main worktree
    """
    if not _is_git_repo(repo):
        raise error.Abort(_("worktree command requires a git repository"))

    if opts.get("json"):
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            cwd=repo.root,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise error.Abort(_("git worktree list failed: %s") % result.stderr.strip())

        worktrees = _parse_porcelain_output(result.stdout, repo.root)
        ui.write(json.dumps(worktrees))
    else:
        result = subprocess.run(
            ["git", "worktree", "list"],
            cwd=repo.root,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise error.Abort(_("git worktree list failed: %s") % result.stderr.strip())

        ui.write(result.stdout)
    return 0


def _parse_porcelain_output(output, main_repo_root):
    """Parse git worktree list --porcelain output into structured data.

    The porcelain format has entries separated by blank lines, with each entry
    containing lines like:
        worktree /path/to/worktree
        HEAD abc123...
        branch refs/heads/main
        (or "detached" for detached HEAD)

    Returns a list of dicts with path, commit, branch (optional), and isMain fields.
    """
    worktrees = []
    current = {}

    for line in output.split("\n"):
        line = line.strip()
        if not line:
            if current:
                # Normalize path for comparison
                current_path = os.path.normpath(current.get("path", ""))
                main_path = os.path.normpath(main_repo_root)
                current["isMain"] = current_path == main_path
                worktrees.append(current)
                current = {}
            continue

        if line.startswith("worktree "):
            current["path"] = line[9:]
        elif line.startswith("HEAD "):
            current["commit"] = line[5:]
        elif line.startswith("branch "):
            # Extract branch name from refs/heads/...
            branch_ref = line[7:]
            if branch_ref.startswith("refs/heads/"):
                current["branch"] = branch_ref[11:]
            else:
                current["branch"] = branch_ref
        elif line == "detached":
            # Detached HEAD, no branch
            pass

    # Don't forget the last entry if output doesn't end with blank line
    if current:
        current_path = os.path.normpath(current.get("path", ""))
        main_path = os.path.normpath(main_repo_root)
        current["isMain"] = current_path == main_path
        worktrees.append(current)

    return worktrees


@subcmd(
    "remove",
    [
        ("f", "force", False, _("force removal even with local changes")),
    ],
    _("PATH"),
)
def remove_cmd(ui, repo, path=None, **opts):
    """remove a working tree

    Removes a git worktree. Use --force to remove a worktree with
    uncommitted changes.

    Examples::

        sl wt remove ../my-repo-feature-x
        sl wt remove --force ../my-repo-feature-x
    """
    if not _is_git_repo(repo):
        raise error.Abort(_("worktree command requires a git repository"))

    if path is None:
        raise error.Abort(_("path is required"))

    force = ["--force"] if opts.get("force") else []
    result = subprocess.run(
        ["git", "worktree", "remove", path] + force,
        cwd=repo.root,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise error.Abort(_("git worktree remove failed: %s") % result.stderr.strip())

    ui.status(_("removed worktree %s\n") % path)
    return 0


def _is_git_repo(repo):
    """Check if the repository is a git repository."""
    git_dir = os.path.join(repo.root, ".git")
    return os.path.exists(git_dir)


def _get_copy_patterns(ui):
    """Get file patterns to copy from config or defaults."""
    custom = ui.configlist("worktree", "copyfiles")
    return custom if custom else DEFAULT_COPY_PATTERNS


def _get_copy_dirs(ui):
    """Get directories to copy from config or defaults."""
    custom = ui.configlist("worktree", "copydirs")
    return custom if custom else DEFAULT_COPY_DIRS


def _copy_untracked_files(ui, repo, dest):
    """Copy untracked config files using CoW when possible."""
    source = repo.root
    copied_any = False

    # Copy individual files
    for pattern in _get_copy_patterns(ui):
        src = os.path.join(source, pattern)
        if os.path.exists(src):
            dst = os.path.join(dest, pattern)
            # Create parent directories if needed
            dst_dir = os.path.dirname(dst)
            if dst_dir and not os.path.exists(dst_dir):
                os.makedirs(dst_dir)
            _cp_cow(src, dst)
            ui.note(_("copied %s\n") % pattern)
            copied_any = True

    # Copy directories (node_modules, etc.) with CoW
    for dirname in _get_copy_dirs(ui):
        src = os.path.join(source, dirname)
        if os.path.isdir(src):
            dst = os.path.join(dest, dirname)
            ui.status(_("copying %s (CoW)...\n") % dirname)
            _cp_cow(src, dst)
            copied_any = True

    if copied_any:
        ui.note(_("finished copying untracked files\n"))


def _cp_cow(src, dst):
    """Copy with copy-on-write support (APFS on macOS, reflink on Linux).

    Tries in order:
    1. macOS: cp -Rc (APFS clone)
    2. Linux: cp -R --reflink=auto (Btrfs/XFS)
    3. Fallback: shutil copy
    """
    if platform.system() == "Darwin":
        # macOS APFS copy-on-write
        result = subprocess.run(["cp", "-Rc", src, dst], capture_output=True)
        if result.returncode == 0:
            return
    else:
        # Linux reflink (Btrfs, XFS)
        result = subprocess.run(
            ["cp", "-R", "--reflink=auto", src, dst],
            capture_output=True,
        )
        if result.returncode == 0:
            return

    # Fallback to regular copy
    if os.path.isdir(src):
        shutil.copytree(src, dst)
    else:
        shutil.copy2(src, dst)


def create_worktree_for_commit(ui, repo, commit_hex, name=None, no_copy=False):
    """Helper function to create a worktree programmatically.

    This is used by other commands like `sl pr get --wt`.

    Args:
        ui: The UI object
        repo: The repository object
        commit_hex: The full commit hash to checkout
        name: Optional name for the worktree (defaults to short hash)
        no_copy: If True, skip copying untracked files

    Returns:
        The path to the created worktree
    """
    commit_short = commit_hex[:8]

    if name is None:
        name = commit_short

    repo_basename = os.path.basename(repo.root)
    wt_dirname = f"{repo_basename}-{name}"
    wt_path = os.path.join(os.path.dirname(repo.root), wt_dirname)

    if os.path.exists(wt_path):
        raise error.Abort(_("path already exists: %s") % wt_path)

    ui.status(_("creating worktree at %s...\n") % wt_path)
    result = subprocess.run(
        ["git", "worktree", "add", wt_path, commit_hex, "--detach"],
        cwd=repo.root,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise error.Abort(_("git worktree add failed: %s") % result.stderr.strip())

    if not no_copy:
        _copy_untracked_files(ui, repo, wt_path)

    ui.status(_("created worktree at %s\n") % wt_path)
    ui.status(_("  cd %s\n") % wt_path)

    return wt_path
