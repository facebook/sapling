# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from .. import error
from ..i18n import _
from . import sparseutil


def _restricted_filter_paths(ui):
    return ui.configlist("pathacl", "tent-filter-paths", [])


def _restricted_filters(repo):
    restricted_filter_paths = _restricted_filter_paths(repo.ui)
    if not restricted_filter_paths:
        return

    enabled_profiles = sparseutil.enabled_profiles(repo)
    filter_restricted_paths = repo.ui.configbool(
        "subtree", "filter-restricted-paths", True
    )
    for restricted_filter_path in restricted_filter_paths:
        is_enabled = restricted_filter_path in enabled_profiles
        yield (
            restricted_filter_path,
            is_enabled,
            is_enabled and filter_restricted_paths,
        )


def _load_restricted_filter(repo, curr_ctx, restricted_filter_path):
    try:
        return sparseutil.load_sparse_profile(repo, curr_ctx, restricted_filter_path)
    except error.ManifestLookupError:
        return None


def _warn_or_abort_restricted_path(
    ui,
    from_path,
    to_path,
    op_name,
    should_filter_restricted_paths,
    abort_by_default,
):
    if should_filter_restricted_paths:
        warn_restricted_paths_omitted(ui, from_path)
        return

    prompt_warning_or_abort(
        ui, from_path, to_path, op_name, abort_by_default=abort_by_default
    )


def validate_path_acl(
    repo, from_paths, to_paths, curr_ctx, filter_path=None, op_name="copy"
):
    ui = repo.ui
    for (
        restricted_filter_path,
        is_filter_enabled,
        should_filter_restricted_paths,
    ) in _restricted_filters(repo):
        if filter_path == restricted_filter_path:
            # restricted paths will be filtered out by the filter (sparse) profile
            continue

        if is_filter_enabled and op_name == "copy":
            # restricted paths will be filtered out by the sparse profile
            continue

        restricted_filter = _load_restricted_filter(
            repo, curr_ctx, restricted_filter_path
        )
        if restricted_filter is None:
            continue

        raw_config, matcher = restricted_filter
        _, exclude = raw_config.toincludeexclude()

        for from_path, to_path in zip(from_paths, to_paths):
            if from_path == to_path:
                continue

            if contains_restricted_data(
                from_path, exclude, matcher
            ) and matcher.matchfn(to_path):
                _warn_or_abort_restricted_path(
                    ui,
                    from_path,
                    to_path,
                    op_name,
                    should_filter_restricted_paths,
                    abort_by_default=is_filter_enabled,
                )


def validate_files_acl(repo, src_files, dest, curr_ctx, op_name="copy"):
    """Validate the ACL of copy/move patterns."""
    ui = repo.ui
    for (
        restricted_filter_path,
        is_filter_enabled,
        should_filter_restricted_paths,
    ) in _restricted_filters(repo):
        if is_filter_enabled and op_name in ("copy", "move"):
            # restricted paths should not exist in the working copy
            continue

        restricted_filter = _load_restricted_filter(
            repo, curr_ctx, restricted_filter_path
        )
        if restricted_filter is None:
            continue

        _, matcher = restricted_filter

        if not matcher.matchfn(dest):
            continue
        for src in src_files:
            if not matcher.matchfn(src):
                _warn_or_abort_restricted_path(
                    ui,
                    src,
                    dest,
                    op_name,
                    should_filter_restricted_paths,
                    abort_by_default=is_filter_enabled,
                )


def warn_restricted_paths_omitted(ui, path):
    ui.warn(
        _("restricted data was omitted from path '%s'; result may be incomplete\n")
        % path,
        notice="warning",
    )


def prompt_warning_or_abort(ui, from_path, to_path, op_name, abort_by_default=False):
    default_prompt_tmpl = _(
        "WARNING: You are attempting to %s restricted data to an unrestricted location:\n"
        " * from-path: %s (contains restricted data)\n"
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
            f"copying restricted path to an unrestricted path is not allowed",
            hint=hint,
        )
    elif ui.promptchoice(prompt_msg, default=1) != 0:
        hint = extra_hint
        raise error.Abort(
            f"copying restricted path to an unrestricted path is not allowed",
            hint=hint,
        )


def contains_restricted_data(from_path, restricted_paths, unrestricted_matcher) -> bool:
    """Check if the from_path contains the restricted data.

    - unrestricted_matcher is generated from a tent_filter sparse profile

    "contains" has two meanings:
    1. from_path is inside a restricted path
    2. from_path is a parent of a restricted path
    """
    if not unrestricted_matcher.matchfn(from_path):
        return True
    prefix = from_path + "/"
    for p in restricted_paths:
        if p.startswith(prefix):
            return True
    return False
