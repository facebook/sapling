from __future__ import absolute_import, print_function
import struct
from mercurial import (
    bdiff,
    mpatch,
)

def test1(a, b):
    d = bdiff.bdiff(a, b)
    c = a
    if d:
        c = mpatch.patches(a, [d])
    if c != b:
        print("***", repr(a), repr(b))
        print("bad:")
        print(repr(c)[:200])
        print(repr(d))

def test(a, b):
    print("***", repr(a), repr(b))
    test1(a, b)
    test1(b, a)

test("a\nc\n\n\n\n", "a\nb\n\n\n")
test("a\nb\nc\n", "a\nc\n")
test("", "")
test("a\nb\nc", "a\nb\nc")
test("a\nb\nc\nd\n", "a\nd\n")
test("a\nb\nc\nd\n", "a\nc\ne\n")
test("a\nb\nc\n", "a\nc\n")
test("a\n", "c\na\nb\n")
test("a\n", "")
test("a\n", "b\nc\n")
test("a\n", "c\na\n")
test("", "adjfkjdjksdhfksj")
test("", "ab")
test("", "abc")
test("a", "a")
test("ab", "ab")
test("abc", "abc")
test("a\n", "a\n")
test("a\nb", "a\nb")

#issue1295
def showdiff(a, b):
    bin = bdiff.bdiff(a, b)
    pos = 0
    while pos < len(bin):
        p1, p2, l = struct.unpack(">lll", bin[pos:pos + 12])
        pos += 12
        print(p1, p2, repr(bin[pos:pos + l]))
        pos += l
showdiff("x\n\nx\n\nx\n\nx\n\nz\n", "x\n\nx\n\ny\n\nx\n\nx\n\nz\n")
showdiff("x\n\nx\n\nx\n\nx\n\nz\n", "x\n\nx\n\ny\n\nx\n\ny\n\nx\n\nz\n")
# we should pick up abbbc. rather than bc.de as the longest match
showdiff("a\nb\nb\nb\nc\n.\nd\ne\n.\nf\n",
         "a\nb\nb\na\nb\nb\nb\nc\n.\nb\nc\n.\nd\ne\nf\n")

print("done")

def testfixws(a, b, allws):
    c = bdiff.fixws(a, allws)
    if c != b:
        print("*** fixws", repr(a), repr(b), allws)
        print("got:")
        print(repr(c))

testfixws(" \ta\r b\t\n", "ab\n", 1)
testfixws(" \ta\r b\t\n", " a b\n", 0)
testfixws("", "", 1)
testfixws("", "", 0)

print("done")
