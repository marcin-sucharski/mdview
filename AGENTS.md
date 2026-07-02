# Repository Instructions

This project is a terminal UI application and must be tested inside tmux before
considering TUI-related work complete.

Required verification after behavior changes:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `scripts/integration-tmux.sh`
- `nix flake check`

The tmux integration test should use an isolated tmux socket and must cover
startup rendering, `j/k`, arrow keys, PageUp/PageDown, mouse-wheel scrolling,
resize handling, file reloads, atomic save/rename reloads, and Markdown image
fallback behavior through tmux.
