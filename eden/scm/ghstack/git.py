import re
from typing import List, Pattern

import ghstack.diff
import ghstack.shell
from ghstack.types import GitCommitHash, GitTreeHash

RE_RAW_COMMIT_ID = re.compile(r'^(?P<commit>[a-f0-9]+)$', re.MULTILINE)
RE_RAW_AUTHOR = re.compile(r'^author (?P<author>(?P<name>[^<]+?) <(?P<email>[^>]+)>)',
                           re.MULTILINE)
RE_RAW_PARENT = re.compile(r'^parent (?P<commit>[a-f0-9]+)$', re.MULTILINE)
RE_RAW_TREE = re.compile(r'^tree (?P<tree>.+)$', re.MULTILINE)
RE_RAW_COMMIT_MSG_LINE = re.compile(r'^    (?P<line>.*)$', re.MULTILINE)


class CommitHeader(object):
    """
    Represents the information extracted from `git rev-list --header`
    """
    # The unparsed output from git rev-list --header
    raw_header: str

    def __init__(self, raw_header: str):
        self.raw_header = raw_header

    def _search_group(self, regex: Pattern[str], group: str) -> str:
        m = regex.search(self.raw_header)
        assert m
        return m.group(group)

    def tree(self) -> GitTreeHash:
        return GitTreeHash(self._search_group(RE_RAW_TREE, "tree"))

    def title(self) -> str:
        return self._search_group(RE_RAW_COMMIT_MSG_LINE, "line")

    def commit_id(self) -> GitCommitHash:
        return GitCommitHash(
            self._search_group(RE_RAW_COMMIT_ID, "commit"))

    def parents(self) -> List[GitCommitHash]:
        return [GitCommitHash(m.group("commit"))
                for m in RE_RAW_PARENT.finditer(self.raw_header)]

    def author(self) -> str:
        return self._search_group(RE_RAW_AUTHOR, "author")

    def author_name(self) -> str:
        return self._search_group(RE_RAW_AUTHOR, "name")

    def author_email(self) -> str:
        return self._search_group(RE_RAW_AUTHOR, "email")

    def commit_msg(self) -> str:
        return '\n'.join(
            m.group("line")
            for m in RE_RAW_COMMIT_MSG_LINE.finditer(self.raw_header))


def split_header(s: str) -> List[CommitHeader]:
    return list(map(CommitHeader, s.split("\0")[:-1]))


class GitPatch(ghstack.diff.Patch):
    h: CommitHeader

    def __init__(self, h: CommitHeader):
        self.h = h

    def apply(self, sh: ghstack.shell.Shell, base_tree: GitTreeHash) -> GitTreeHash:
        expected_tree = sh.git("rev-parse", self.h.commit_id() + "~^{tree}")
        assert expected_tree == base_tree, \
            "expected_tree = {}, base_tree = {}".format(expected_tree, base_tree)
        return self.h.tree()


def parse_header(s: str, github_url: str) -> List[ghstack.diff.Diff]:
    def convert(h: CommitHeader) -> ghstack.diff.Diff:
        parents = h.parents()
        if len(parents) != 1:
            raise RuntimeError(
                "The commit {} has {} parents, which makes my head explode.  "
                "`git rebase -i` your diffs into a stack, then try again."
                .format(h.commit_id(), len(parents)))
        return ghstack.diff.Diff(
            title=h.title(),
            summary=h.commit_msg(),
            oid=h.commit_id(),
            source_id=h.tree(),
            pull_request_resolved=ghstack.diff.PullRequestResolved.search(h.raw_header, github_url),
            patch=GitPatch(h)
        )
    return list(map(convert, split_header(s)))
