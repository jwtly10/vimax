# vimax

A WIP Emacs inspired buffer-driven GUI text editor with native Vim bindings

https://github.com/user-attachments/assets/dfe658a8-a3fa-4aeb-8eec-7580aa0dffe0

## Features

- **Modal editing** — Normal, Insert, Visual, Visual Line, Command, Search modes
- **Buffers** - almost everything is a `text buffer`, to allow shared context so vim motion & syntax highlighting etc work the same everywhere
- **Vim motions** — `hjkl`, `w/b/e`, `0/^/$`, `gg/G`, `f/F/t/T` with `;/,` repeat, `Ctrl-d/u` half-page scroll
- **Operators + text objects** — `d/c/y` compose with motions and text objects (`iw/aw`, `i(/a(`, `i"/a"`, etc.)
- **Count prefixes** — `5j`, `3dd`, etc.
- **Incremental search** — `/` with match highlighting, `n/N`, search history
- **Command mode** — `:w`, `:q`, `:wq`, `:e`, `:bn`, `:bp`, `:bd`, `:vs`, `:sp`, `:close`, command history
- **Multiple buffers** — open, switch, close
- **Window splits** — vertical and horizontal, binary tree layout, `Ctrl-w h/j/k/l` navigation
- **Fuzzy picker** — `nucleo`-backed, with live preview. `<leader>bb` buffers, `<leader>pf` files (respects `.gitignore`), `<leader>xx` diagnostics
- **Syntax highlighting** — tree-sitter via `tree-house`, Rust grammar bundled, One Dark theme
- **LSP** — document sync, go to definition (`gd`), references (`gr`), implementation (`gi`), declaration (`gD`), hover (`K`), completions with fuzzy filtering
- **Diagnostics** — inline underlines, scrollbar markers, modeline counts, ghost text, diagnostics picker
- **Completions** — LSP-driven popup, trigger characters, debounced, `Tab/Enter` accept
- **Info panel** — hover docs and diagnostic details, preview and pinned modes
- **Jump list** — `Ctrl-o` back, `Ctrl-i` forward
- **Undo/redo** — with edit grouping
- **Auto-indent** — copies leading whitespace, adds indent after `{/(/[`
- **System clipboard** — `Cmd-c/v/x`
- **Scrollbar** — custom widget with drag, click-to-jump, search and diagnostic markers
- **GUI** - gpu rendered widgets, without the limitations of a terminal emulator

## Building

```
cargo run
cargo run -- path/to/file.rs
RUST_LOG=vimax=trace cargo run
```
