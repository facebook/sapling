# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from .. import error
from ..i18n import _


def validate_path_acl(repo, from_paths, to_paths, curr_ctx, op_name="copy"):
    from sapling.ext import sparse

    ui = repo.ui
    acl_file = ui.config("pathacl", "tent-filter-path")
    if not acl_file:
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
            default_prompt_tmpl = _(
                "WARNING: You are attempting to %s protected data to an unprotected location:\n"
                " * from-path: %s (contains protected data)\n"
                " * to-path: %s\n"
                "Do you still wish to continue (y/n)? $$ &Yes $$ &No"
            )
            prompt_tmpl = ui.config(
                "pathacl", "prompt-warning-template", default_prompt_tmpl
            )
            prompt_msg = prompt_tmpl % (op_name, from_path, to_path)
            if ui.promptchoice(prompt_msg, default=1) != 0:
                hint = ui.config("pathacl", "path-validation-hint")
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
