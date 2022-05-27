# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Translate .py tests back to .t tests

Resulting code might need extra manual editing to work.

    python3.9 -m testutil.rtranslate test-foo-t.py

Requires libcst.
"""

from __future__ import annotations

import ast
import os
import sys
import textwrap
import traceback
from typing import Optional, Tuple, Union, List

import libcst as cst


def translatepath(path, hgmv=True):
    with open(path) as f:
        code = f.read()
    body = "#debugruntest-compatible\n" + translatebody(code)
    newpath = path.replace("-t.py", ".t")
    with open(newpath, "wb") as f:
        f.write(body.encode(errors="replace"))
    if hgmv:
        os.system("hg mv --after %s %s" % (path, newpath))
    # update features.py
    with open("features.py", "r") as f:
        features = f.read()
        features = features.replace(os.path.basename(path), os.path.basename(newpath))
    with open("features.py", "w") as f:
        f.write(features)


patcache = {}


class Matched(dict):
    def __getattr__(self, name):
        return self.get(name, None)

    def __bool__(self):
        return True


def matchcode(patcode: str, tree: cst.CSTNode) -> Optional[Matched]:
    pattree = patcache.get(patcode)
    if pattree is None:
        mod = cst.parse_module(patcode)
        assert len(mod.children) == 1
        pattree = patcache[patcode] = mod.children[0]
    return matchtree(pattree, tree)


debugflag = False


def debug(v):
    if debugflag:
        print(v)


def matchtree(pat: cst.CSTNode, tree: cst.CSTNode, comments=None) -> Optional[Matched]:
    debug(f"MATCHTREE {pat} {tree}")
    # pattern like "_a"
    if type(pat) is cst.Name and pat.value.startswith("_"):
        return Matched({pat.value[1:]: tree})

    # space match
    if isspace(pat) and isspace(tree):
        return Matched()

    # other patterns - check type and children recursively
    if type(pat) is not type(tree):
        debug("TYPE MISMATCH")
        return None

    # block match
    if type(pat) is cst.IndentedBlock:
        s = tocode(pat).strip()
        if s.startswith("_") and s[1:].isalnum():
            return Matched({s[1:]: tree})

    # pattern like "# _a" - capture comment
    if type(pat) is cst.EmptyLine and pat.comment.value.startswith("# _"):
        return Matched({pat.comment.value[3:]: tree})

    patchildren, _patcomments = cleanup(getchildren(pat))
    treechildren, treecomments = cleanup(getchildren(tree))
    if len(patchildren) != len(treechildren):
        debug("CHILDREN LENGTH MISMATCH")
        return None

    matched = Matched()
    for patchild, treechild in zip(patchildren, treechildren):
        childmatched = matchtree(patchild, treechild, treecomments)
        if childmatched is None:
            debug("CHILD MISMATCH")
            return None
        matched.update(childmatched)
    if comments is not None:
        comments += treecomments
    else:
        matched["comments"] = treecomments
    debug("CHILD MATCHED")
    if not patchildren and tocode(pat).strip() != tocode(tree).strip():
        debug("SELF MISMATCH")
        return None
    return matched


def getchildren(tree: cst.CSTNode) -> List[cst.CSTNode]:
    if isinstance(tree, cst.If):
        # do not flatten test, or body
        children = [tree.test, tree.body]
        if tree.orelse:
            children.append(tree.orelse)
    else:
        children = tree.children
    return children


def cleanup(children: List[cst.CSTNode]) -> Tuple[List[cst.CSTNode], List[cst.CSTNode]]:
    """return (trees without space or comments, trees of non-empty comments)"""
    result = []
    comments = []
    for c in children:
        if isspaceorcomment(c):
            if tocode(c).strip():
                comments.append(c)
            continue
        result.append(c)
    return result, comments


def isspaceorcomment(tree: cst.CSTNode) -> bool:
    return isinstance(
        tree,
        (
            cst.EmptyLine,
            cst.TrailingWhitespace,
            cst.ParenthesizedWhitespace,
            cst.SimpleWhitespace,
        ),
    )


def isspace(tree: cst.CSTNode) -> bool:
    return not tocode(tree).strip()


def tocode(tree: Union[cst.CSTNode, List[cst.CSTNode]]) -> str:
    if not isinstance(tree, list):
        tree = [tree]
    return cst.Module(tree).code


def evalstr(tree: cst.SimpleString) -> str:
    if not isinstance(tree, cst.SimpleString):
        raise AssertionError(f"{tocode(tree)} is not a simple string")
    value = ast.literal_eval(tree.value)
    # some commands might be double quoted
    try:
        value2 = ast.literal_eval(value)
        if isinstance(value2, str):
            value = value2
    except Exception:
        pass
    return str(value)


def parsefeatures(tree: cst.CSTNode) -> List[str]:
    features = ast.literal_eval(tocode(tree))
    if isinstance(features, int):
        features = str(features)
    if isinstance(features, str):
        features = [features]
    if not isinstance(features, list):
        raise TypeError(f"{tocode(tree)} is not a list of features")
    return features


def translatebody(code: str) -> str:
    mod = cst.parse_module(code)
    out = []
    for tree in mod.children:
        # preserve empty lines
        for l in getattr(tree, "leading_lines", ()):
            if isspaceorcomment(l):
                if out and out[-1] != "\n":
                    out.append("\n")

        def match(code):
            if isinstance(code, list):
                for i, c in enumerate(code):
                    m = match(c)
                    if m:
                        m["index"] = i
                        return m
                return None
            m = matchcode(code, tree)
            if m:
                # preserve comments
                comments = m.get("comments", ())
                for c in comments:
                    line = tocode(c).lstrip()
                    if line != "# noqa: F401\n":
                        out.append(line)
                if comments and out and out[-1] != "\n":
                    out.append("\n")
            return m

        def appendrefout(m: Matched):
            if not m.z:
                return
            refout = evalstr(m.z)
            if refout:
                refout = textwrap.indent(
                    textwrap.dedent(refout.lstrip("\n")), "  ", lambda l: True
                )
                out.append(refout + "\n")

        if m := match("# _a"):
            out.append(tocode(m.a))
            pass
        elif m := match(
            [
                "if feature.check(_a):\n    _b",
                "if feature.check(_a):\n    _b\nelse:\n    _c",
            ]
        ):
            features = parsefeatures(m.a)
            out.append(f'#if {" ".join(features)}\n')
            ifbodycode = tocode(m.b).lstrip("\n")
            ifbodycode = textwrap.dedent(ifbodycode)
            out.append(translatebody(ifbodycode))
            if m.c:
                out.append("#else\n")
                elsebodycode = tocode(m.c).lstrip("\n")
                elsebodycode = textwrap.dedent(elsebodycode)
                out.append(elsebodycode)
            out.append(f"#endif\n")
        elif m := match("feature.require(_a)"):
            features = parsefeatures(m.a)
            out.append(f'#require {" ".join(features)}\n')
        elif m := match("from __future__ import absolute_import"):
            pass
        elif m := match("from testutil.dott import feature, sh, testtmp  # noqa: F401"):
            pass
        elif m := match("sh % _a"):
            out.append(f"  $ {evalstr(m.a)}\n")
        elif m := match(["sh % _a > _c", "sh % _a >> _c"]):
            op = (">", ">>")[m.index]
            out.append(f"  $ {evalstr(m.a)} {op} {evalstr(m.c)}\n")
        elif m := match("sh % _a << open(_b).read() == _z"):
            out.append(f"  $ {evalstr(m.a)} < {evalstr(m.b)}\n")
            appendrefout(m)
        elif m := match(["sh % _a << _b", "(sh % _a << _b)", "sh % _a << _b == _z"]):
            out.append(f"  $ {evalstr(m.a)} << 'EOS'\n")
            heredoc = evalstr(m.b).lstrip("\n")
            out.append(textwrap.indent(heredoc, "  > ", lambda l: True))
            out.append("  > EOS\n")
            appendrefout(m)
        elif m := match(
            [
                "sh % _a << _b > _c",
                "(sh % _a << _b > _c)",
                "sh % _a << _b >> _c",
                "(sh % _a << _b >> _c)",
            ]
        ):
            op = (">", ">>")[m.index // 2]
            out.append(f"  $ {evalstr(m.a)} {op} {evalstr(m.c)} << 'EOF'\n")
            heredoc = evalstr(m.b).lstrip("\n")
            out.append(textwrap.indent(heredoc, "  > ", lambda l: True))
            out.append("  > EOF\n")
        elif m := match(["sh % _a == _z", "sh % _a | _b == _z"]):
            cmds = [evalstr(m[s]) for s in ["a", "b"] if s in m]
            cmd = " | ".join(cmds)
            out.append(f"  $ {cmd}\n")
            appendrefout(m)
        else:
            if out and out[-1] != "\n":
                out.append("\n")
            out += [
                "# BEGIN OF NOT TRANSLATED\n",
                tocode(tree).lstrip(),
                "# END OF NOT TRANSLATED\n\n",
            ]

    return "".join(out)


def main(argv):
    for path in argv:
        if path.endswith("-t.py"):
            print("Translating %s" % path)
            try:
                translatepath(path)
            except Exception:
                traceback.print_exc()


if __name__ == "__main__":
    main(sys.argv[1:])
