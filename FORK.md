# herdr 0.9.0 with underline colors

This branch is herdr v0.9.0 plus a fix for
[herdrdev/herdr#1178](https://github.com/herdrdev/herdr/issues/1178): underline
colors (SGR 58) are dropped between a pane and the attached client. An
undercurl drawn by Neovim, a spell checker or an LSP shows up in the text's own
color instead of the color the program asked for.

## Reproduce

In a pane, run:

```sh
printf '\e[4:3m\e[58:2::255:0:0mX\e[0m\n'
```

On stock 0.9.0 the `X` gets a curly underline drawn in the foreground color. On
this branch the underline is red. `herdr pane read <pane> --format ansi` shows
the color is already in the server's terminal state on stock 0.9.0; it is lost
on the way to the client.

## How the color is carried

Each cell now keeps its underline color, and the client emits it as SGR 58 in
the colon sub-parameter form: `58:2::r:g:b` for RGB colors and `58:5:n` for
palette and named colors. It is the form herdr already uses for curly underlines
(`4:3`) and advertises as `Setulc`. In the semicolon form (`58;2;r;g;b`), a
terminal without SGR 58 reads the values after 58 as SGR codes of their own; in
the colon form they stay inside the one parameter.

Pane surfaces travel to clients in the `shell.surface.v1` endpoint codec, which
is frozen: `AGENTS.md` says named core codecs are immutable, and that a wire
change needs a new codec name with the old one kept as a fallback. So the color
is not added to v1. This branch adds a second surface codec, `shell.surface.v2`:

- A v2 pane surface or patch frame is the unchanged v1 payload followed by a
  trailer listing `(cell index, color)` for the cells that have an underline
  color. Every other message is identical under v1 and v2.
- The client offers `[shell.surface.v2, shell.surface.v1]` in its endpoint
  hello. The server picks v2 only when it is offered and names its choice in the
  welcome's existing `surface_codec` field.
- The v1 bytes, the endpoint generation (1) and `PROTOCOL_VERSION` are
  unchanged.

## Compatibility with stock 0.9.0

For shell clients, which receive pane surfaces:

- Stock client, this server: the client offers only v1 and receives the same
  surface bytes a stock server sends. It works; underline colors are not shown.
- This client, stock server: the server answers v1, which this client accepts.
  It works; underline colors are not shown.
- This client and this server: v2, with underline colors.

Direct terminal clients, which connect with `TerminalHello` instead of the
endpoint handshake, receive `Terminal` frames that the server encodes as ANSI
itself. From this server they carry SGR 58, stock clients included. Their wire
format is unchanged; only the ANSI inside it gains the color.

## Build

Upstream's `CLAUDE.md` says herdr requires Zig 0.15.2 for the vendored
libghostty-vt, and `vendor/libghostty-vt/build.zig.zon` sets
`minimum_zig_version = "0.15.2"`. This branch was built and tested with 0.15.2
only. If the `zig` on your `PATH` is another version, point the build at a
0.15.2 binary without changing `PATH`; `build.rs` reads `ZIG` and falls back to
`zig`:

```sh
ZIG=/path/to/zig-0.15.2/zig cargo build --release
```

## Tests

`just check` runs upstream's suite together with this branch's tests. The v1
fallback is covered in both directions. A v1-only client reaching this server is
an in-process handshake test
(`client_shell_handshake_keeps_surface_v1_for_a_v1_only_client`). This client
reaching a stock server is a unit test of the welcome parser against the frozen
generation-1 welcome fixture
(`frozen_generation_one_welcome_selects_surface_codec_v1`). Nothing outside the
repository is needed.

## History

The earlier patch for herdr 0.8.x (branch `fix/underline-color-wire`, commit
`f03d6209`) wrote SGR 58 in the semicolon form, `58;2;r;g;b`; this series writes
the colon form.
