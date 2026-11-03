Run systematic **technical** quality checks on a desktop app (`desktop`) and generate a comprehensive report. Don't fix issues; document them for other commands to address.

This is a code-level audit, not a design critique. Audit from source and from screenshots of the running window. Score against [desktop.md](desktop.md), plus [gpui.md](gpui.md) when the toolkit is GPUI. Read them before scoring if Setup hasn't already. The report skeleton mirrors [audit.md](audit.md); keep the two in sync when changing it.

**The detector depends on the toolkit** (`DESKTOP_TOOLKIT` in the context output). GPUI: run `{{scripts_path}}/impeccable detect --json <source dirs>` and fold its findings into the dimensions below; it reads source only. When the app's debug build is running with impeccable-gpui-probe built with `snapshot`, also run `{{scripts_path}}/impeccable gpui snapshot` in each appearance and at the smallest supported window size, and fold those findings in: they come from what the window painted, so a snapshot finding outranks a source finding on the same line, and a clean snapshot covers contrast and cut-off text for that state. Without it, contrast, overflow, and density still need the screenshots. A web shell (Electron, Tauri, NW.js): run the web detector the way [audit.md](audit.md) does for desktop apps. Any other toolkit: no detector applies; do not present an empty run as clean.

## Diagnostic Scan

Run comprehensive checks across 5 dimensions. Score each dimension 0-4 using the criteria below.

### 1. Accessibility (keyboard and screen readers)

**Check for**:
- **Keyboard reach**: actions only a mouse can trigger; hover-only controls with no keyboard or context-menu path
- **Focus**: no visible focus ring, Tab order that jumps around, focus lost after a dialog or popover closes
- **Screen-reader labels**: icon-only buttons without accessible names or tooltips; custom controls without roles or state (VoiceOver, Narrator, Orca)
- **Contrast**: text, meta text, and placeholders under 4.5:1 in either appearance
- **Text scaling**: layouts that clip at larger OS text sizes or 200% display scale
- **Reduce Motion ignored**: large or looping animation with no reduced alternative

**Score 0-4**: 0=Unusable without a mouse, 1=Major gaps (core actions unreachable, no focus ring), 2=Partial (reachable, focus or labels inconsistent), 3=Good (minor gaps), 4=Excellent (fully keyboard-operable, labelled, contrast holds in both appearances)

### 2. Performance

**Check for**:
- **Slow first window**: heavy work before the first frame; blank window on launch
- **Unvirtualized lists**: long lists or tables laid out in full (GPUI: `children(..)` instead of `uniform_list` / `list`)
- **Main-thread work**: file, network, or parsing work blocking input or rendering; dropped frames while scrolling or resizing
- **Resize cost**: relayout storms or flicker while dragging the window edge or a splitter
- **Memory**: unbounded caches, images decoded at full size for thumbnails

**Score 0-4**: 0=Janky everywhere, 1=Major problems (blocking work, unvirtualized lists), 2=Partial, 3=Good (minor improvements possible), 4=Excellent (instant window, smooth resize and scroll)

### 3. Appearance & Theming

**Check for**:
- **Hard-coded colors**: raw color literals in views instead of theme tokens
- **Broken appearance**: dark or light mode missing, inverted instead of designed, or not following the OS setting
- **Accent overuse**: the accent painted on large surfaces instead of state (selection, focus, primary action)
- **Off-platform materials**: hand-rolled blur where vibrancy, Mica, or Acrylic is expected
- **Type drift**: sizes off the scale, secondary text under 11 px, a generic default face

**Score 0-4**: 0=Hard-coded everything, 1=Minimal tokens, 2=Partial (tokens exist, inconsistently used), 3=Good (minor hard-coded values), 4=Excellent (tokens throughout, both appearances first-class)

### 4. Platform Conformance (CRITICAL)

Score against the loaded platform reference(s), including the desktop slop test. **Check for**:
- **Website in a frame**: phone-scale padding, a centered column in a wide window, hamburger menus hiding commands
- **Missing desktop affordances**: no shortcuts, no context menus, no menu bar on macOS, no command palette in a command-heavy app
- **Broken window chrome**: custom title bars that cannot drag, maximize, or snap; content under the traffic lights or caption buttons
- **Selection and editing**: Cmd/Ctrl-click, Shift-click, arrow keys, undo/redo, and clipboard not behaving like the OS
- **Dialog conventions**: modal used for non-blocking messages, button order against the OS, Enter/Esc not wired

**Score 0-4**: 0=Web port (nothing desktop), 1=Heavy violations (3-4 kinds), 2=Some (1-2 noticeable), 3=Mostly conformant (subtle issues), 4=Fully native (a desktop power user trusts every window)

### 5. Window & Input Adaptivity

**Check for**:
- **Minimum size**: no minimum window size, or layouts that break before reaching it
- **Wide windows**: content stretched across an ultrawide window, or stuck in a narrow column
- **Panes**: splitters that cannot resize, sizes not remembered, panes that never collapse at narrow widths
- **Display scale**: blurry icons or vanishing hairlines at 100% / 150% / 200%
- **Multiple windows and monitors**: state lost when a second window opens, windows restored off-screen

**Score 0-4**: 0=One window size only, 1=Major breakage (narrow or wide windows broken), 2=Partial, 3=Good (minor edge cases), 4=Excellent (holds from minimum size to ultrawide, at every scale)

## Generate Report

### Audit Health Score

| # | Dimension | Score | Key Finding |
|---|-----------|-------|-------------|
| 1 | Accessibility | ? | [most critical issue or "--"] |
| 2 | Performance | ? | |
| 3 | Appearance & Theming | ? | |
| 4 | Platform Conformance | ? | |
| 5 | Window & Input Adaptivity | ? | |
| **Total** | | **??/20** | **[Rating band]** |

**Rating bands**: 18-20 Excellent (minor polish), 14-17 Good (address weak dimensions), 10-13 Acceptable (significant work needed), 6-9 Poor (major overhaul), 0-5 Critical (fundamental issues)

### Platform Conformance Verdict
**Start here.** Pass/fail: does this read as a desktop app or a website in a window? List specific violations. Be brutally honest.

### Executive Summary
- Audit Health Score: **??/20** ([rating band])
- Total issues found (count by severity: P0/P1/P2/P3)
- Top 3-5 critical issues
- Recommended next steps

### Detailed Findings by Severity

Tag every issue with **P0-P3 severity**:
- **P0 Blocking**: Prevents task completion. Fix immediately
- **P1 Major**: Significant difficulty or platform-convention violation. Fix before release
- **P2 Minor**: Annoyance, workaround exists. Fix in next pass
- **P3 Polish**: Nice-to-fix, no real user impact. Fix if time permits

For each issue, document:
- **[P?] Issue name**
- **Location**: Window or view, file, line
- **Category**: Accessibility / Performance / Theming / Conformance / Adaptivity
- **Impact**: How it affects users
- **Guideline**: The desktop or toolkit rule it violates (if applicable)
- **Recommendation**: How to fix it
- **Suggested command**: Which command to use (prefer: {{available_commands}})

### Patterns & Systemic Issues

Identify recurring problems that indicate systemic gaps rather than one-off mistakes:
- "Raw color literals appear in 15+ views, should come from the theme"
- "Icon-only toolbar buttons have no tooltips or shortcuts anywhere"

### Positive Findings

Note what's working well: good practices to maintain and replicate.

## Recommended Actions

List recommended commands in priority order (P0 first, then P1, then P2):

1. **[P?] `{{command_prefix}}command-name`**: Brief description (specific context from audit findings)
2. **[P?] `{{command_prefix}}command-name`**: Brief description (specific context)

**Rules**: Only recommend commands from: {{available_commands}}. Map findings to the most appropriate command. End with `{{command_prefix}}impeccable polish` as the final step if any fixes were recommended.

After presenting the summary, tell the user:

> You can ask me to run these one at a time, all at once, or in any order you prefer.
>
> Re-run `{{command_prefix}}impeccable audit` after fixes to see your score improve.

Keep P3s few: a long tail of minor issues buries the ones that matter.

**Avoid**:
- Report issues without explaining impact (why does this matter?)
- Provide generic recommendations (be specific and actionable)
- Skip positive findings (celebrate what works)
- Forget to prioritize (everything can't be P0)
- Report false positives without verification
