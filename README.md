# ZedX

A fork of [Zed](https://github.com/zed-industries/zed), the high-performance, multiplayer code editor from the creators of [Atom](https://github.com/atom/atom) and [Tree-sitter](https://github.com/tree-sitter/tree-sitter).

ZedX is maintained by [ccbox.app](https://ccbox.app) and includes additional features inspired by the [ccbox](https://github.com/anthropic-tools/ccbox) TUI session manager for AI coding agents.

## Modifications from upstream Zed

- Version suffix: all builds carry the `ccbox` pre-release identifier (e.g., `0.229.0-ccbox+dev`)
- Rebranded as **ZedX** with `app.ccbox.ZedX*` bundle identifiers and channel-specific app names
- macOS dev installs bundle as `ZedX.app`
- **Terminal as first-class center tabs** with terminal-attached planning notes (see below)

### Terminal tabs in the center pane

Upstream Zed opens terminals exclusively in the bottom dock panel. ZedX promotes terminals to first-class editor tabs:

- **Terminals open as center tabs** -- `NewTerminal` and `NewTerminalInDir` create a `TerminalView` directly in the active editor pane instead of the dock panel, so terminals sit alongside files as regular tabs.
- **Welcome screen shortcut** -- the "Get Started" section on the welcome page includes a "New Terminal" entry.
- **Manual close confirmation only** -- closing a terminal tab from the tab `X` shows a "Close this terminal?" confirmation prompt, while shell-driven exits such as `Ctrl+D` close automatically without prompting.
- The `Item` trait gains a `close_message()` method that any item type can implement to show a custom close confirmation dialog.

### Terminal-attached planning notes

Each terminal session can own exactly one attached planning note:

- **Attached to the terminal session** -- the note is opened from the terminal tab header, shown in a right-hand split, and hidden whenever that terminal tab is no longer the selected terminal.
- **Persistent scratch buffer** -- the note uses Zed's internal unsaved-editor persistence, so the Markdown buffer survives app restarts without forcing a file save.
- **Restored by terminal context** -- the note is keyed to the terminal session and working directory, then reattached automatically when the same terminal is restored.
- **Markdown editing** -- planning notes open as Markdown buffers for planning, checklists, and agent coordination.

## Building

### macOS

```sh
make build    # cargo build --release
make install  # bundle and install the macOS app
```

See also:
- [Building Zed for macOS](./docs/src/development/macos.md)
- [Building Zed for Linux](./docs/src/development/linux.md)
- [Building Zed for Windows](./docs/src/development/windows.md)

## Upstream

This project tracks [zed-industries/zed](https://github.com/zed-industries/zed) `main` branch. Upstream changes are rebased periodically.

## Licensing

ZedX is distributed under the same licenses as Zed:

- **GPL-3.0-or-later** for the editor ([LICENSE-GPL](./LICENSE-GPL))
- **AGPL-3.0-or-later** for server components ([LICENSE-AGPL](./LICENSE-AGPL))
- **Apache-2.0** for client libraries ([LICENSE-APACHE](./LICENSE-APACHE))

Original copyright: 2022-2025 Zed Industries, Inc.

License information for third party dependencies must be correctly provided for CI to pass. See upstream [CONTRIBUTING.md](./CONTRIBUTING.md) for details on `cargo-about` compliance.
