# mdview

`mdview` is a small Rust terminal UI for previewing Markdown on Linux.

It renders strict CommonMark in the alternate screen, supports keyboard and
mouse scrolling, shows local images in iTerm2-compatible terminals, and reloads
automatically when the viewed file changes.

## Features

- Read-only Markdown viewing with no editor mode
- Keyboard and mouse scrolling
- Automatic reloads on file changes and atomic saves
- Light-theme-friendly terminal styling
- iTerm2 inline images through tmux/SSH where supported
- Nix flake package and development shell

## Usage

```sh
nix run . -- examples/basic.md
```

Inside the viewer:

- `j`, `Down`, or mouse wheel down scroll down
- `k`, `Up`, or mouse wheel up scroll up
- `PageDown` and `PageUp` scroll by a page
- `g` and `G` jump to the top or bottom
- `q`, `Esc`, or `Ctrl-C` quit

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
