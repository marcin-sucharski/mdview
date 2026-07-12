# mdview

`mdview` is a small Rust terminal UI for previewing Markdown on Linux.

It renders strict CommonMark in the alternate screen, supports keyboard and
optional mouse scrolling, shows local images in iTerm2-compatible terminals, and reloads
automatically when the viewed file changes.

## Features

- Read-only Markdown viewing with no editor mode
- Keyboard scrolling, with optional mouse wheel scrolling
- Normal terminal/tmux text selection by default inside tmux
- Optional in-app mouse text selection with OSC 52 clipboard copy
- Case-insensitive in-view search with highlighted matches
- Automatic reloads on file changes and atomic saves
- Light-theme-friendly terminal styling
- GitHub-style pipe table rendering with alignment and wrapping
- Highlighted fenced code blocks for JSON, HTTP, SQL/PostgreSQL, XML, and plain text
- iTerm2 inline images through tmux/SSH where supported
- Nix flake packages and development shells for x86_64-linux and aarch64-linux

## Usage

```sh
nix run . -- examples/basic.md
```

Inside the viewer:

- `j` or `Down` scroll down
- `k` or `Up` scroll up
- `PageDown` and `PageUp` scroll by a page
- `g` and `G` jump to the top or bottom
- `/` opens search, `Enter` searches, and empty `Enter` clears search
- `n` moves to the next search match and `p` moves to the previous one
- `q`, `Esc`, or `Ctrl-C` quit

Mouse reporting is disabled by default inside tmux so normal tmux/iTerm2 text
selection works. Outside tmux, mouse wheel scrolling is enabled by default.
Set `MDVIEW_MOUSE=wheel` to force wheel scrolling, `MDVIEW_MOUSE=on` to enable
wheel scrolling plus in-app drag selection, or `MDVIEW_MOUSE=off` to disable
mouse reporting. With `MDVIEW_MOUSE=on`, drag with the left mouse button to
select text, then press `y`, `c`, `Enter`, or right click to copy through OSC
52.

## iTerm2 Images Through tmux

Local Markdown images are rendered with the iTerm2 inline image protocol when
support is detected. Through normal tmux passthrough, this setting may be
needed:

```tmux
set -g allow-passthrough on
```

When running over SSH inside tmux, iTerm2-specific environment variables may
not be forwarded. In tmux or screen sessions, mdview follows `imgcat` and emits
tmux-wrapped iTerm2 image sequences anyway. Set `MDVIEW_IMAGES=off` to disable
image output, or `MDVIEW_IMAGES=force` to force iTerm2 image output outside
tmux when detection is wrong.

## Development

```sh
nix develop
cargo fmt --check
cargo clippy -- -D warnings
cargo test
scripts/integration-tmux.sh
nix flake check
```
