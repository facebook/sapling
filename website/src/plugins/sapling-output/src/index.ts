/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// See project README.md for how to use this plugin in markdown.
const Convert = require('ansi-to-html');
const {execFile} = require('child_process');
const fs = require('fs');
const path = require('path');
const {promisify} = require('util');
const tmp = require('tmp-promise');

// Header of `.t` to setup environment.
// Force color output by setting ui.color.
// Set TEST_PROD_CONFIGS to load fancy templates and stuff.
const EXAMPLE_HEADER = String.raw`
# hide begin
  $ cat >> "$HGRCPATH" << 'EOF'
  > [ui]
  > color=always
  > [init]
  > prefer-git=false
  > [templatealias]
  > sl_difflink="{if(github_pull_request_number,'#{github_pull_request_number}')}"
  > github_pull_request_number="{get(extras,'pr_number')}"
  > github_pr_state="{get(extras,'pr_state')}"
  > github_pull_request_status_check_rollup="{get(extras,'pr_status')}"
  > EOF
  $ export TEST_PROD_CONFIGS=1
  $ export HGCOLORS=16
  $ export SL_COLORS=16
# hide end
`

// Compatibility for internal systems where sl is not installed but hg can be
// used as an alternative.
const HG_COMPAT_HEADER = String.raw`
# hide begin
  $ sl() {
  >   HGIDENTITY=sl hg "$@"
  > }
# hide end
`

// Replace each line with lineFunc(line).
// Skip a line if lineFunc(line) returns null.
function processLines(text: string, lineFunc: (line: string) => string | null): string {
  const lines = text.split("\n");
  const outputLines = lines.map(lineFunc).filter(line => line != null);
  return outputLines.join("\n") + "\n";
};

// Change example code in mdx to a `.t` test format by:
// - adding EXAMPLE_HEADER.
// - adding "  " prefix.
// - ensure "# hide end" is not treated as "reference output" in `.t`.
function processInput(text: string): string {
  let firstLine = true;
  return processLines(text, (line) => {
    let newLine = line;
    if (line.startsWith("# hide")) {
      // Insert an empty line separator so it does not get treated as output.
      newLine = `\n  ${line}`;
    } else if (line.length > 0) {
      newLine = `  ${line}`;
    }
    if (firstLine) {
      let header = EXAMPLE_HEADER;
      // Currently debugruntest only knows about "hg".
      header += HG_COMPAT_HEADER;
      newLine = header + newLine;
      firstLine = false;
    }
    return newLine;
  });
};

// Clean up output (in `.t` format) by:
// - removing `# hide` lines, and blocks between `# hide begin` and
//   `# hide end`.
// - removing leading empty lines.
function processOutput(output: string): string {
  let firstLine = true;
  let hiding = 0;
  return processLines(output, (line) => {
    // Skip leading spaces.
    if (firstLine && line === "") {
      return null;
    }
    firstLine = false;
    // Handle "# hide" comments.
    if (line.includes('# hide begin')) {
      hiding += 1;
    }
    if (hiding <= 0 && !line.includes(' # hide') && line.startsWith('  ')) {
      return line.substr(2);
    }
    if (line.includes('# hide end')) {
      hiding -= 1;
    }
    return null;
  });
};

const COLORS = {
  // Campbell
  0: "#0C0C0C",
  1: "#C50F1F",
  2: "#13A10E",
  3: "#C19C00",
  4: "#0037DA",
  5: "#881798",
  6: "#3A96DD",
  7: "#CCCCCC",
  8: "#767676",
  9: "#E74856",
  10: "#16C60C",
  11: "#F9F1A5",
  12: "#3B78FF",
  13: "#B4009E",
  14: "#61D6D6",
  15: "#F2F2F2",
}

// Render `example` (in .t test format without "  " prefix) into HTML.
async function renderExample(example: string): Promise<string> {
  // Prepare input.
  const isDebug = process.env.MDX_SAPLING_OUTPUT_DEBUG != null;
  const tmpDir = await tmp.dir({ prefix: "mdx-sapling-output", unsafeCleanup: !isDebug });
  const examplePath = path.join(tmpDir.path, 'example.t');
  const data = EXAMPLE_HEADER + processInput(example);
  await fs.promises.writeFile(examplePath, data);

  // Use `debugruntest --fix` to fill the output.
  const exePath = getSaplingCLI();
  try {
    await promisify(execFile)(exePath, ["debugruntest", "-q", "--fix", examplePath]);
  } catch (e) {
    // exitcode = 1 means "at least one mismatch", which is expected.
    // @ts-ignore
    if (e.code !== 1) {
      throw e;
    }
  }

  // Convert to HTML.
  const rawOutput = await fs.promises.readFile(examplePath, {"encoding": "utf8"});
  const output = processOutput(rawOutput);
  const convert = new Convert({colors: COLORS});
  const body = convert.toHtml(output);
  const html = `<pre class="sapling-example">${body}</pre>`;

  // Cleanup.
  if (!isDebug) {
    tmpDir.cleanup();
  }

  return html;
}

interface Node {
  type: string
  value: string
}

interface CodeNode extends Node {
  type: 'code'
  lang: string
}

module.exports = function (options: any) {
  return async function (ast: any) {
    const {visit} = await import('unist-util-visit');

    // See https://github.com/mdx-js/specification#mdxast for specification of
    // "node".

    const nodes: Node[] = [];

    // Collect nodes.
    visit(ast, 'code', (node: CodeNode) => {
      if (node.lang === 'with-output') {
        nodes.push(node);
      }
    });

    // Process them.
    await Promise.all(nodes.map(async (node: Node) => {
      node.type = 'html';
      node.value = await renderExample(node.value);
    }));
  };
};

function getSaplingCLI(): string {
  let cli;
  // @fb-only
  if (!cli) {
    cli = 'sl';
  }
  return cli;
}
