# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import List, Optional, Tuple

from .gh_submit import Repository

_HORIZONTAL_RULE = "---"
_SAPLING_FOOTER_MARKER = "[//]: # (BEGIN SAPLING FOOTER)"


def create_pull_request_title_and_body(
    commit_msg: str,
    pr_numbers_and_num_commits: List[Tuple[int, int]],
    pr_numbers_index: int,
    repository: Repository,
    title: Optional[str] = None,
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
    [//]: # (BEGIN SAPLING FOOTER)
    Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack]({reviewstack_url}).
    * #1
    * #2 (2 commits)
    * __->__ #42
    * #4

    Add trailing whitespace to commit_msg and ensure it is preserved.
    >>> commit_msg += '\n\n'
    >>> title, body = create_pull_request_title_and_body(
    ...     commit_msg,
    ...     pr_numbers_and_num_commits,
    ...     pr_numbers_index,
    ...     contributor_repo,
    ... )
    >>> print(body.replace(reviewstack_url, "{reviewstack_url}"))
    The original commit message.
    Second line of message.
    <BLANKLINE>
    ---
    [//]: # (BEGIN SAPLING FOOTER)
    Stack created with [Sapling](https://sapling-scm.com). Best reviewed with [ReviewStack]({reviewstack_url}).
    * #1
    * #2 (2 commits)
    * __->__ #42
    * #4

    Disable reviewstack message:
    >>> title, body = create_pull_request_title_and_body(commit_msg, pr_numbers_and_num_commits,
    ...     pr_numbers_index, contributor_repo, reviewstack=False)
    >>> print(body)
    The original commit message.
    Second line of message.
    <BLANKLINE>
    ---
    [//]: # (BEGIN SAPLING FOOTER)
    * #1
    * #2 (2 commits)
    * __->__ #42
    * #4

    Single commit stack:
    >>> title, body = create_pull_request_title_and_body("Foo", [(1, 1)], 0, contributor_repo)
    >>> print(body.replace(reviewstack_url, "{reviewstack_url}"))
    Foo
    """
    owner, name = repository.get_upstream_owner_and_name()
    pr = pr_numbers_and_num_commits[pr_numbers_index][0]

    if title is None:
        title = firstline(commit_msg)
        body = commit_msg[len(title) + 1 :]
    body = _strip_stack_information(commit_msg)
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
        if not body.endswith("\n"):
            body += "\n"
        body += "\n".join([_HORIZONTAL_RULE, _SAPLING_FOOTER_MARKER] + extra)
    return title, body


_STACK_ENTRY = re.compile(r"^\* (__->__ )?#([1-9]\d*).*$")

# Pair where the first value is True if this entry was noted as the "current"
# one with the "__->__" marker; otherwise, False.
# The second value is the pull request number for this entry.
_StackEntry = Tuple[bool, int]


def parse_stack_information(body: str) -> List[_StackEntry]:
    r"""
    With sapling stack footer marker:
    >>> reviewstack_url = "https://reviewstack.dev/facebook/sapling/pull/42"
    >>> body = (
    ...     'The original commit message.\n' +
    ...     'Second line of message.\n' +
    ...     '---\n' +
    ...     '[//]: # (BEGIN SAPLING FOOTER)\n' +
    ...     'Stack created with [Sapling](https://sapling-scm.com). ' +
    ...     f'Best reviewed with [ReviewStack]({reviewstack_url}).\n' +
    ...     '* #1\n' +
    ...     '* #2 (2 commits)\n' +
    ...     '* __->__ #42\n' +
    ...     '* #4\n')
    >>> parse_stack_information(body)
    [(False, 1), (False, 2), (True, 42), (False, 4)]

    Without sapling stack footer marker (legacy):
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
    """
    in_stack_list = False
    stack_entries = []
    for line in body.splitlines():
        if _line_has_stack_list_marker(line):
            in_stack_list = True
        elif in_stack_list:
            match = _STACK_ENTRY.match(line)
            if match:
                arrow, number = match.groups()
                stack_entries.append((bool(arrow), int(number, 10)))
            else:
                # This must be the end of the list.
                break
    return stack_entries


def _line_has_stack_list_marker(line: str) -> bool:
    # we're still looking at the "Stack created with [Sapling]" text for backward compatibility
    return line == _SAPLING_FOOTER_MARKER or line.startswith(
        "Stack created with [Sapling]"
    )


_SAPLING_FOOTER_WITH_HRULE = re.compile(
    re.escape(_HORIZONTAL_RULE) + r"\r?\n" + re.escape(_SAPLING_FOOTER_MARKER),
    re.MULTILINE,
)


def _strip_stack_information(body: str) -> str:
    r"""
    Footer marker joined with \n
    >>> body = (
    ...     'The original commit message.\n' +
    ...     'Second line of message.\n' +
    ...     '---\n' +
    ...     '[//]: # (BEGIN SAPLING FOOTER)\n' +
    ...     '* #1\n' +
    ...     '* #2 (2 commits)\n' +
    ...     '* __->__ #42\n' +
    ...     '* #4\n')
    >>> _strip_stack_information(body)
    'The original commit message.\nSecond line of message.\n'

    Footer marker joined with \r\n. If the user edits the pull request body
    on github.com, GitHub will rewrite the line endings to \r\n.
    >>> _strip_stack_information(body.replace('\n', '\r\n'))
    'The original commit message.\r\nSecond line of message.\r\n'

    If the footer marker appears multiple times in the body, everything will
    be stripped after the first occurrence.
    >>> _strip_stack_information(body + body)
    'The original commit message.\nSecond line of message.\n'
    """
    return re.split(_SAPLING_FOOTER_WITH_HRULE, body, maxsplit=1)[0]


def _format_stack_entry(
    pr_number: int,
    pr_number_index: int,
    current_pr_index: int,
    num_commits: int,
) -> str:
    line = (
        f"* #{pr_number}"
        if current_pr_index != pr_number_index
        else f"* __->__ #{pr_number}"
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
