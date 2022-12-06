import DrawDagExample from "@site/src/components/DrawDagExample";

# DrawDag

DrawDag provides an intuitive way to create commit graph for tests.

## Background

When creating tests, we often need to create a repo with a particular layout.
For example, to create a linear graph with three commits, we could use the
following sequence of commands:

```sl-shell-example
$ sl commit -m A
$ sl commit -m B
$ sl commit -m C
```

If the graph is nonlinear, extra commands such as merge and goto are needed:

```sl-shell-example
$ sl commit -m A
$ sl commit -m B
$ sl goto -q '.^'
$ sl commit -m C
$ sl merge -q 'desc(B)'
$ sl commit -m D
```

As you can see, creating the desired graph shape via writing out a sequence of
commands is tedious, potentially error prone, and not immediately obvious what
the resulting graph looks like.

To help aid people in writing tests (and those reviewing the tests!), we've
created DrawDag to simply and intuitively create repos with the desired shape.


## DrawDag language

DrawDag is a domain specific language to describe a DAG (Directed Acyclic Graph).

### Basic

In this example, the DrawDag code looks like a hexagon and generates the graph
to the right:

<DrawDagExample showParents={true} initValue={String.raw`
    -B-
   /   \
  A--C--D
   \   /
    E-F
`} />

The DrawDag code forms a 2D matrix of characters. There are three types of
characters:

- Space characters.
- Connect characters: `-`,  `\`, and `/`.
- Name characters: alpha, numeric, and some other characters.

Names define vertexes in the graph. Connect characters define edges in the graph.

If two vertexes are directly connected, the one to the left becomes a parent of
the other vertex. For a commit graph, this behaves like making commits from
left to right.

If a vertex has multiple parents, those parents are sorted in lexicographical
order.

### Name at multiple locations

A single name can be used in multiple locations and will represent the same
vertex in the graph.

For example, the code below uses `C` in two locations to create criss-cross
merges.

<DrawDagExample initValue={String.raw`
  A-C
   \
  B-D
   \
    C
`} />

### Range generation

You can use `..` (or more dots) to generate a range of vertexes and connect
them. This works for simple alphabet names like `A..Z` or numbers like
`A01..A99`:

<DrawDagExample initValue={String.raw`
  A..C...F
      \ /
       K
`} />

The range expansion under the hood works similarly to
[Ruby](https://www.ruby-lang.org/)'s [Range](https://ruby-doc.org/core/Range.html).

### Vertical layout

By default, DrawDag assumes a horizontal layout. You can opt-in the alternative
vertical layout by using `|`, or `:`. It has a few differences:

- `|` is a valid connect character. `-` becomes invalid.
- `:` is used for range generation. `.` becomes a valid name character.

<DrawDagExample initValue={String.raw`
  Z
  :\
  C B
  |/
  A
`} />

Commits are created from bottom to top. This is similar to `sl log -G` output
order.

### Try DrawDag

Try editing the DrawDag code above. We draw the output live in the browser.

## DrawDag in tests

### `.t` integration tests

You can use the `drawdag` shell function in `.t` tests to create commits and
change the repo.

```sl-shell-example
$ drawdag << 'EOS'
>  C
>  |
> B1 B2  # amend: B1 -> B2
>   \|
>    A
> EOS
```

`#` starts a comment till the end of the line. Comments won't be parsed as
DrawDag code but might have other meanings:

- `# A/dir/file = 1`: In commit `A`, update path `dir/file` to content `1`.
- `# amend: X -> Y -> Z`: Mark `Y` as amended from `X`, `Z` as amended from `Y`.
- `# bookmark FOO = A`: Create bookmark `FOO` that points to commit `A`.

You can also use revset expressions to refer to existing commits. For example,
`.` in vertical layout refers to the working parent.

Check `test-drawdag.t` for more examples.

### Rust unit tests

You can use the `drawdag` crate to parse DrawDag code into graph vertexes and
edges.

The `dag` crate might also be useful to run complex queries on a graph, and
render it as ASCII.
