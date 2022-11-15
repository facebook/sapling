---
sidebar_position: 20
---

import {Command, ReviewStackScreenshot, SLCommand} from '@site/elements'

# ReviewStack

ReviewStack [(reviewstack.dev)](https://reviewstack.dev) is a novel user interface for GitHub pull requests with custom support for _stacked changes_. The user experience is inspired by Meta's internal code review tool, but leverages [GitHub's design system](https://primer.style/) to achieve a look and feel that is familiar to GitHub users:

<ReviewStackScreenshot />

To try it out, take the URL for an existing pull request on **github.com** and change the domain to **reviewstack.dev**, so:

https://github.com/bolinfest/monaco-tm/pull/39 on GitHub is available at<br />https://reviewstack.dev/bolinfest/monaco-tm/pull/39 on ReviewStack.

Specifically, ReviewStack recognizes stacks created by <Command name="pr" linkText="sl pr submit" />, <SLCommand name="ghstack" />, or stacks created from a Git repo using [standalone `ghstack`](https://github.com/ezyang/ghstack). When it sees that your pull request is part of such a stack, it provides a dropdown for navigating the commits in the stack. This encourages discussing and approving each change independently while allowing the author to add new commits to the top of the stack without interfering with the existing conversation around the commits on the bottom of the stack.

:::caution

Today, ReviewStack is focused on motivating the conversation about how an ideal _stacked changes_ workflow should work. It is admittedly not as fully-featured or battle-tested (particularly for large pull requests) as GitHub's pull request UI. For these reasons, a pull request in ReviewStack links back to the equivalent page on GitHub so you can quickly make use of any GitHub functionality that is missing on ReviewStack.

:::

## Keyboard Shortcuts

ReviewStack supports the following keyboard shortcuts:

| key                         | action                        |
| --------------------------- | ----------------------------- |
| `shift+N`                   | next PR in stack              |
| `shift+P`                   | previous PR in stack          |
| `ctrl+.` (`cmd+.` on macOS) | toggle timeline view          |
| `alt+A`                     | select Approve Changes action |
| `alt+R`                     | select Request Changes action |
| `alt+C`                     | select Comment action         |
