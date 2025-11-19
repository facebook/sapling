# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from .. import error
from ..i18n import _
from . import sparseutil


def validate_path_acl(
    repo, from_paths, to_paths, curr_ctx, filter_path=None, op_name="copy"
):
    from sapling.ext import sparse

    ui = repo.ui
    acl_file = ui.config("pathacl", "tent-filter-path")
    if not acl_file:
        return

    if filter_path == acl_file:
        # protected paths will be filtered out by the filter (sparse) profile
        return

    is_acl_enabled = sparseutil.is_profile_enabled(repo, acl_file)
    if is_acl_enabled and op_name == "copy":
        # protected paths will be filtered out by the sparse profile
        return

    try:
        raw_content = sparse.getrawprofile(repo, acl_file, curr_ctx.hex())
    except error.ManifestLookupError:
        # the file might not exist
        return

    raw_config = sparse.readsparseconfig(repo, raw_content, filename=acl_file, depth=1)
    include, exclude = raw_config.toincludeexclude()
    matcher = sparse.computesparsematcher(repo, [curr_ctx.rev()], raw_config)

    for from_path, to_path in zip(from_paths, to_paths):
        if from_path == to_path:
            continue

        if contains_protected_data(from_path, exclude, matcher) and matcher.matchfn(
            to_path
        ):
            # abort by default for users don't have access (the ACL file was enabled)
            abort_by_default = is_acl_enabled
            prompt_warning_or_abort(
                ui, from_path, to_path, op_name, abort_by_default=abort_by_default
            )


def validate_files_acl(repo, src_files, dest, curr_ctx, op_name="copy"):
    """Validate the ACL of copy/move patterns."""
    assert op_name in ("copy", "move")
    ui = repo.ui
    acl_file = ui.config("pathacl", "tent-filter-path")
    if not acl_file:
        return

    if sparseutil.is_profile_enabled(repo, acl_file):
        # protected paths should not exist in the working copy
        return

    if acl_file not in curr_ctx:
        # the acl file does not exist
        return

    unprotected_matcher = sparseutil.load_sparse_profile_matcher(
        repo, curr_ctx, acl_file
    )

    if not unprotected_matcher.matchfn(dest):
        return
    for src in src_files:
        if not unprotected_matcher.matchfn(src):
            prompt_warning_or_abort(ui, src, dest, op_name)


def prompt_warning_or_abort(ui, from_path, to_path, op_name, abort_by_default=False):
    default_prompt_tmpl = _(
        "WARNING: You are attempting to %s protected data to an unprotected location:\n"
        " * from-path: %s (contains protected data)\n"
        " * to-path: %s"
    )
    confirm_question = "Do you still wish to continue (y/n)? $$ &Yes $$ &No"
    prompt_tmpl = ui.config("pathacl", "prompt-warning-template", default_prompt_tmpl)
    prompt_warning = prompt_tmpl % (op_name, from_path, to_path)
    prompt_msg = prompt_warning + "\n" + confirm_question
    extra_hint = ui.config("pathacl", "path-validation-hint")

    if abort_by_default:
        hint = f"{prompt_warning}\n{extra_hint}" if extra_hint else prompt_warning
        raise error.Abort(
            f"copying protected path to an unprotected path is not allowed",
            hint=hint,
        )
    elif ui.promptchoice(prompt_msg, default=1) != 0:
        hint = extra_hint
        raise error.Abort(
            f"copying protected path to an unprotected path is not allowed",
            hint=hint,
        )


def contains_protected_data(from_path, protected_paths, unprotected_matcher) -> bool:
    """Check if the from_path contains the protected data.

    - unprotected_matcher is generated from a tent_filter sparse profile

    "contains" has two meanings:
    1. from_path is inside a protected path
    2. from_path is a parent of a protected path
    """
    if not unprotected_matcher.matchfn(from_path):
        return True
    prefix = from_path + "/"
    for p in protected_paths:
        if p.startswith(prefix):
            return True
    return False
