# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import List, Tuple

from .gh_submit import Repository

_HORIZONTAL_RULE = "---"


def create_pull_request_title_and_body(
    commit_msg: str,
    pr_numbers_and_num_commits: List[Tuple[int, int]],
    pr_numbers_index: int,
    repository: Repository,
    reviewstack: bool = True,
) -> Tuple[str, str]:
    r"""Returns (title, body) for the pull request.

    >>> commit_msg = 'The original commit message.\nSecond line of message.'
    >>> pr_numbers_and_num_commits = [(1, 1), (2, 2), (42, 1), (4, 1)]
    >>> pr_numbers_index = 2
    >>> upstream_repo = Repository(hostname="github.com", id="abcd=", owner="facebook", name="sapling", default_branch="main", is_fork=False)
    >>> contributor_repo = Repository(hostname="github.com", id="efgh=", owner="keith", name="sapling", default_branch="main", is_fork=True, upstream=upstream_repo)
    >>> title, body = create_pull_request_title_and_body(
    ...     commit_msg,
    ...     pr_numbers_and_num_commits,
    ...     pr_numbers_index,
    ...     contributor_repo,
    ... )
    >>> print(title)
    The original commit message.
    >>> reviewstack_url = "https://reviewstack.dev/facebook/sapling/pull/42"
    >>> print(body.replace(reviewstack_url, "{reviewstack_url}"))
    The original commit message.
    Second line of message.
    ---
    Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack]({reviewstack_url}).
    * #1
    * #2 (2 commits)
    * #42 \u21E6
    * #4

    Disable reviewstack message:
    >>> title, body = create_pull_request_title_and_body(commit_msg, pr_numbers_and_num_commits,
    ...     pr_numbers_index, contributor_repo, reviewstack=False)
    >>> print(body)
    The original commit message.
    Second line of message.
    ---
    * #1
    * #2 (2 commits)
    * #42 \u21E6
    * #4

    Single commit stack:
    >>> title, body = create_pull_request_title_and_body("Foo", [(1, 1)], 0, contributor_repo)
    >>> print(body.replace(reviewstack_url, "{reviewstack_url}"))
    Foo
    """
    owner, name = repository.get_upstream_owner_and_name()
    pr = pr_numbers_and_num_commits[pr_numbers_index][0]
    title = firstline(commit_msg)
    body = commit_msg
    extra = []
    if len(pr_numbers_and_num_commits) > 1:
        if reviewstack:
            reviewstack_url = f"https://reviewstack.dev/{owner}/{name}/pull/{pr}"
            review_stack_message = f"Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack]({reviewstack_url})."
            extra.append(review_stack_message)
        bulleted_list = "\n".join(
            _format_stack_entry(pr_number, index, pr_numbers_index, num_commits)
            for index, (pr_number, num_commits) in enumerate(pr_numbers_and_num_commits)
        )
        extra.append(bulleted_list)
    if extra:
        body = "\n".join([body, _HORIZONTAL_RULE] + extra)
    return title, body


_STACK_ENTRY = re.compile(r"^\* (__->__ )?#([1-9]\d*)( \u21E6)?.*$")

# Pair where the first value is True if this entry was noted as the "current"
# one with the "__->__" marker; otherwise, False.
# The second value is the pull request number for this entry.
_StackEntry = Tuple[bool, int]


def parse_stack_information(body: str) -> List[_StackEntry]:
    r"""
    >>> reviewstack_url = "https://reviewstack.dev/facebook/sapling/pull/42"
    >>> body = (
    ...     'The original commit message.\n' +
    ...     'Second line of message.\n' +
    ...     '---\n' +
    ...     'Stack created with [Sapling](https://sapling-scm.com). ' +
    ...     f'Best reviewed with [ReviewStack]({reviewstack_url}).\n' +
    ...     '* #1\n' +
    ...     '* #2 (2 commits)\n' +
    ...     '* __->__ #42\n' +
    ...     '* #4\n')
    >>> parse_stack_information(body)
    [(False, 1), (False, 2), (True, 42), (False, 4)]
    >>> body = (
    ...     'The original commit message.\n' +
    ...     'Second line of message.\n' +
    ...     '---\n' +
    ...     'Stack created with [Sapling](https://sapling-scm.com). ' +
    ...     '* #1\n' +
    ...     '* #2 (2 commits)\n' +
    ...     '* #42 \u21E6\n' +
    ...     '* #4\n')
    >>> parse_stack_information(body)
    [(False, 1), (False, 2), (True, 42), (False, 4)]
    """
    is_prev_line_hr = False
    in_stack_list = False
    stack_entries = []
    for line in body.splitlines():
        if in_stack_list:
            match = _STACK_ENTRY.match(line)
            if match:
                arrow, number = match.groups()
                stack_entries.append((bool(arrow), int(number, 10)))
            else:
                # This must be the end of the list.
                break
        elif is_prev_line_hr:
            if line.startswith("Stack created with [Sapling]"):
                in_stack_list = True
            is_prev_line_hr = False
        elif line.rstrip() == _HORIZONTAL_RULE:
            is_prev_line_hr = True
    return stack_entries


def _format_stack_entry(
    pr_number: int,
    pr_number_index: int,
    current_pr_index: int,
    num_commits: int,
) -> str:
    line = (
        f"* #{pr_number}"
        if current_pr_index != pr_number_index
        else f"* #{pr_number} \u21E6"
    )
    if num_commits > 1:
        line += f" ({num_commits} commits)"
    return line


_EOL_PATTERN = re.compile(r"\r?\n")
_MAX_FIRSTLINE_LEN = 120


def firstline(msg: str) -> str:
    r"""Returns the "first line" of a commit message to use for the title of a
    pull request.

    >>> firstline("foobar")
    'foobar'
    >>> firstline("foo\nbar")
    'foo'
    >>> firstline("foo\r\nbar")
    'foo'
    >>> firstline("x" * (_MAX_FIRSTLINE_LEN + 1)) == "x" * _MAX_FIRSTLINE_LEN
    True
    """
    match = _EOL_PATTERN.search(msg)
    end = match.start() if match else len(msg)
    end = min(end, _MAX_FIRSTLINE_LEN)
    return msg[:end]
