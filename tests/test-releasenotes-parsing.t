#require fuzzywuzzy

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > releasenotes=
  > EOF

Bullet point with a single item spanning a single line

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * Bullet point item with a single line
  > EOF
  section: feature
    bullet point:
      paragraph: Bullet point item with a single line

Bullet point that spans multiple lines.

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * Bullet point with a paragraph
  >   that spans multiple lines.
  > EOF
  section: feature
    bullet point:
      paragraph: Bullet point with a paragraph that spans multiple lines.

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * Bullet point with a paragraph
  >   that spans multiple lines.
  > 
  >   And has an empty line between lines too.
  >   With a line cuddling that.
  > EOF
  section: feature
    bullet point:
      paragraph: Bullet point with a paragraph that spans multiple lines.
      paragraph: And has an empty line between lines too. With a line cuddling that.

Multiple bullet points. With some entries being multiple lines.

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * First bullet point. It has a single line.
  > 
  > * Second bullet point.
  >   It consists of multiple lines.
  > 
  > * Third bullet point. It has a single line.
  > EOF
  section: feature
    bullet point:
      paragraph: First bullet point. It has a single line.
    bullet point:
      paragraph: Second bullet point. It consists of multiple lines.
    bullet point:
      paragraph: Third bullet point. It has a single line.

Bullet point without newline between items

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * First bullet point
  > * Second bullet point
  >   And it has multiple lines
  > * Third bullet point
  > * Fourth bullet point
  > EOF
  section: feature
    bullet point:
      paragraph: First bullet point
    bullet point:
      paragraph: Second bullet point And it has multiple lines
    bullet point:
      paragraph: Third bullet point
    bullet point:
      paragraph: Fourth bullet point

Sub-section contents are read

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > First Feature
  > -------------
  > 
  > This is the first new feature that was implemented.
  > 
  > And a second paragraph about it.
  > 
  > Second Feature
  > --------------
  > 
  > This is the second new feature that was implemented.
  > 
  > Paragraph two.
  > 
  > Paragraph three.
  > EOF
  section: feature
    subsection: First Feature
      paragraph: This is the first new feature that was implemented.
      paragraph: And a second paragraph about it.
    subsection: Second Feature
      paragraph: This is the second new feature that was implemented.
      paragraph: Paragraph two.
      paragraph: Paragraph three.

Multiple sections are read

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > * Feature 1
  > * Feature 2
  > 
  > Bug Fixes
  > =========
  > 
  > * Fix 1
  > * Fix 2
  > EOF
  section: feature
    bullet point:
      paragraph: Feature 1
    bullet point:
      paragraph: Feature 2
  section: fix
    bullet point:
      paragraph: Fix 1
    bullet point:
      paragraph: Fix 2

Mixed sub-sections and bullet list

  $ hg debugparsereleasenotes - << EOF
  > New Features
  > ============
  > 
  > Feature 1
  > ---------
  > 
  > Some words about the first feature.
  > 
  > Feature 2
  > ---------
  > 
  > Some words about the second feature.
  > That span multiple lines.
  > 
  > Other Changes
  > -------------
  > 
  > * Bullet item 1
  > * Bullet item 2
  > EOF
  section: feature
    subsection: Feature 1
      paragraph: Some words about the first feature.
    subsection: Feature 2
      paragraph: Some words about the second feature. That span multiple lines.
    bullet point:
      paragraph: Bullet item 1
    bullet point:
      paragraph: Bullet item 2
