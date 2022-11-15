import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';

# LineLog

LineLog is an implementation of [interleaved deltas](https://en.wikipedia.org/wiki/Interleaved_deltas)..
It provides conflict-free stack editing ability. It is used by the `absorb`
command.

## Bytecode

LineLog uses a bytecode format that is interpreted to produce content.
There are 4 instructions:

| Name | Operand 1 | Operand 2 | Meaning                                   |
|------|-----------|-----------|-------------------------------------------|
| JGE  | `Rev`     | `Addr`    | Jump to `Addr` if `Current Rev` >= `Rev`  |
| J    |  0        | `Addr`    | Jump to `Addr` unconditionally            |
| JL   | `Rev`     | `Addr`    | Jump to `Addr` if `Current Rev` < `Rev`   |
| LINE | `Rev`     | `Line`    | Append the `Line + 1`-th line in `Rev`    |
| END  | -         | -         | Stop execution                            |

Instructions are fixed-sized. The opcode takes 2 bits. `J` and `JGE` share the
same opcode. Operand 1 takes 30 bits. Operand 2 takes 32 bits.

## Interpretation

### Example

It is easier to understand with an example. Given a file with 3 revisions:

<div className="row">
  <div className="col col--4">
Rev 1
<pre>
a<br/>
b<br/>
c
</pre>
  </div>
  <div className="col col--4">
Rev 2: Inserted 2 lines.
<pre>
a<br/>
b<br/>
1<br/>
2<br/>
c
</pre>
  </div>
  <div className="col col--4">
Rev 3: Deleted 2 lines.
<pre>
a<br/>
2<br/>
c
</pre>
  </div>
</div>

It can be encoded in LineLog bytecode like:

```
# Addr: Instruction
     0: JL   1 8
     1: LINE 1 0
     2: JGE  3 6
     3: LINE 1 1
     4: JL   2 7
     5: LINE 2 2
     6: LINE 2 3
     7: LINE 1 2
     8: END
```

To check out a specified revision, set `Current Rev` to the revision to check
out, then execute the instructions from the beginning.

Here are the steps to check out each revision:

<Tabs>
  <TabItem value="r0" label="Rev 0">
    <ul>
      <li>Address 0: JL 1 8: Jump to address 8, because Current Rev (0) &lt; 1.</li>
      <li>Address 8: END: Stop execution. The content is empty.</li>
    </ul>
  </TabItem>
  <TabItem value="r1" label="Rev 1" default>
    <ul>
      <li>Address 0: JL 1 8: Do nothing, because Current Rev (1) is not &lt; 1.</li>
      <li>Address 1: LINE 1 0: Append the first line from rev 1 ("a").</li>
      <li>Address 2: JGE 3 6: Do nothing, because 1 is not &ge; 3.</li>
      <li>Address 3: LINE 1 1: Append the second line from rev 1 ("b").</li>
      <li>Address 4: JL 2 7: Jump to address 7, because 1 &lt; 2.</li>
      <li>Address 7: LINE 1 2: Append the third line from rev 1 ("c").</li>
      <li>Address 8: END: Stop. The final content is "abc".</li>
    </ul>
  </TabItem>
  <TabItem value="r2" label="Rev 2">
    <ul>
      <li>Address 0: JL 1 8: Do nothing, because Current Rev (2) is not &lt; 1.</li>
      <li>Address 1: LINE 1 0: Append the first line from rev 1 ("a").</li>
      <li>Address 2: JGE 3 6: Do nothing, because 2 is not &ge; 3.</li>
      <li>Address 3: LINE 1 1: Append the second line from rev 1 ("b").</li>
      <li>Address 4: JL 2 7: Do nothing, because 2 is not &lt; 2.</li>
      <li>Address 5: LINE 2 2: Append the 3rd line from rev 2 ("1").</li>
      <li>Address 6: LINE 2 3: Append the 4th line from rev 2 ("2").</li>
      <li>Address 7: LINE 1 2: Append the third line from rev 1 ("c").</li>
      <li>Address 8: END: Stop. The final content is "ab12c".</li>
    </ul>
  </TabItem>
  <TabItem value="r3" label="Rev 3">
    <ul>
      <li>Address 0: JL 1 8: Do nothing, because Current Rev (3) is not &lt; 1.</li>
      <li>Address 1: LINE 1 0: Append the first line from rev 1 ("a").</li>
      <li>Address 2: JGE 3 6: Jump to address 6, because 3 &ge; 3.</li>
      <li>Address 6: LINE 2 3: Append the 4th line from rev 2 ("2").</li>
      <li>Address 7: LINE 1 2: Append the third line from rev 1 ("c").</li>
      <li>Address 8: END: Stop. The final content is "a2c".</li>
    </ul>
  </TabItem>
</Tabs>

### Checkout and Annotate

Note the lines that are not changed across multiple revisions, such as "a" only
occurs once as `LINE 1 0` in the bytecode. The `LINE` instruction points to the
revision and line that introduces the line. By tracking the operands of `LINE`
instructions in addition to line contents, LineLog could also produce the
`annotate` (also called `blame`) result at the same time.

In LineLog, the checkout and annotate operation are basically the same.

### Range of Revisions

A variation of the interpretation is to treat "Current Rev" as a range, not a
single fixed revision number. More specifically, given an inclusive range from
`minRev` to `maxRev`, treat `JL` as "< `maxRev`", `JGE` as ">= `minRev`". This
can produce all lines that existed in the revision range, in a reasonable order,
like:

    rev 1: a
    rev 1: b
    rev 2: 1
    rev 2: 2
    rev 1: c

### Linear History

LineLog assumes linear history. The revision comparisons are done using direct
integer comparisons. It might be not too difficult to support non-linear
history (i.e.  with merges) by changing the revision comparisons to consider
the graph topology. But that hasn't been attempted due to lack of use-cases so
far.


## Editing LineLog

LineLog provides a method for editing: `replace_lines(brev, a1, a2, b1, b2)`.
It means replacing the line range `[a1, a2)` from the current checkout to line
range `[b1, b2)` in the given `brev` revision. This covers insertion and
deletion too. If `a1` equals to `a2`, it is an insertion. If `b1` equals to
`b2`, it means lines from `a1` to `a2` are deleted in revision `brev`.

This is implemented by appending a block that appends the lines from the
`brev`, and removes lines from `a`. Then change the `LINE` instruction for the
`a1` line to point to the added block.

```
# Before             # After
# Addr: Instruction  # Addr: Instruction
      : ...                : ...
    a1: <a1's LINE>     a1 : J len
  a1+1: ...           a1+1 : ...
      : ...                : ...
    a2: ...             a2 : ...
      : ...                : ...
   len: N/A            len : JL brev p
                           : LINE brev b1
                           : LINE brev b1+1
                           : ...
                           : LINE brev b2-1
                         p : JGE brev a2
                           : <a1's LINE> (copied)
                           : J a1+1
```

To construct LineLog for a file, one needs to run through the contents of revisions
of the file in commit order, calculate diffs for adjacent revisions, and then
feed LineLog the diffs using the `replace_lines` method.

Usually `replace_lines` is used to edit the latest revision. However, it can
also be used to edit past revisions, if past revisions are checked out. This
is how the `absorb` command works under the hood.
