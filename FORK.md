# herdr 0.9.5 with underline colors

This branch is herdr v0.9.5 plus a fix for
[herdrdev/herdr#1178](https://github.com/herdrdev/herdr/issues/1178): underline
colors (SGR 58) are dropped between a pane and the attached client. An
undercurl drawn by Neovim, a spell checker or an LSP shows up in the text's own
color instead of the color the program asked for.

## Reproduce

In a pane, run:

```sh
printf '\e[4:3m\e[58:2::255:0:0mX\e[0m\n'
```

On stock 0.9.5 the `X` gets a curly underline drawn in the foreground color. On
this branch the underline is red. `herdr pane read <pane> --format ansi` shows
the color is already in the server's terminal state on stock 0.9.5; it is lost
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
  color. Every other message is identical under v1 and v2, except the delta
  and scroll payloads described below.
- The client offers `[shell.surface.v2, shell.surface.v1]` in its endpoint
  hello. The server picks v2 only when it is offered and names its choice in the
  welcome's existing `surface_codec` field.
- The v1 bytes, the endpoint generation (1) and `PROTOCOL_VERSION` are
  unchanged.

### The optional surface encodings

Since 0.9.1 (reuse and delta) and 0.9.2 (scroll), upstream can also send pane
cells through three optional encodings that a client turns on with
capabilities in its hello. Under v2 each
of them carries the color too:

- `surface_delta` sends only the changed spans of a recomputed surface, inside
  a payload whose layout is frozen with its message kind. Under v2 the same
  `(cell index, color)` trailer follows that payload, indexing the spans' cells
  and then the popup's cells. A v1 reader rejects the extra bytes.
- `surface_scroll` sends a pane's vertical shift plus one ordinary pane patch
  for the rows that still differ. Under v2 that inner patch is a v2 patch, so
  its cells carry their colors; rows moved by the shift keep the colors the
  client already has.
- `surface_reuse` sends no cells at all. The server uses it only when the new
  cells equal the last ones sent, and that comparison includes underline
  colors, so a color change always goes out on a path that carries it.

## Compatibility with stock 0.9.5

For shell clients, which receive pane surfaces:

- Stock client, this server: the client offers only v1, and every surface,
  patch, delta and scroll frame it receives is a v1 frame. It works; underline
  colors are not shown. This server counts a cell whose only change is its
  underline color as changed, so for such a cell it may send a patch, delta or
  full surface where a stock server sends nothing or a reuse. The client draws
  the same screen either way; only the bytes differ.
- This client, stock server: the server answers v1, which this client accepts.
  It works; underline colors are not shown.
- This client and this server: v2, with underline colors.

Direct terminal clients, which connect with `TerminalHello` instead of the
endpoint handshake, receive `Terminal` frames that the server encodes as ANSI
itself. From this server they carry SGR 58, stock clients included. Their wire
format is unchanged; only the ANSI inside it gains the color.

## Build

herdr 0.9.5's vendored libghostty-vt needs Zig 0.16.0:
`vendor/libghostty-vt/build.zig.zon` sets `minimum_zig_version = "0.16.0"`.
This branch was built and tested with 0.16.0 only. If the `zig` on your `PATH`
is another version, point the build at a 0.16.0 binary without changing `PATH`;
`crates/ghostty-vt/build.rs` reads `ZIG` and falls back to `zig`:

```sh
ZIG=/path/to/zig-0.16.0/zig cargo build --release
```

## Tests

`just check` runs upstream's suite together with this branch's tests. The v1
fallback is covered in both directions. A v1-only client reaching this server is
an in-process handshake test
(`client_shell_handshake_keeps_surface_v1_for_a_v1_only_client`). This client
reaching a stock server is a unit test of the welcome parser against the frozen
generation-1 welcome fixture
(`frozen_generation_one_welcome_selects_surface_codec_v1`). Upstream's frozen
v1 and surface-delta fixtures are unchanged.

The full surface and plain patch, delta and scroll each have a test that drives
the real server and decodes what it sent, in
`src/server/headless/tests/underline_color.rs`; the same file checks that a
connecting client keeps the codec it negotiated. Reuse is tested on the
client's render state, not the whole server, in
`surface_reuse_v2_keeps_underline_colors_and_resends_a_color_change`. The
client's own drawing has tests in `render_ansi.rs`: two neighbouring cells that
differ only in underline color both get their color, and a cell whose only
change is its color (set, changed or cleared) is redrawn. The client's side of
the codec has its own test in `src/client/tests/mod.rs`
(`shell_client_shows_underline_colors_from_full_surfaces_and_deltas`): a shell
client handshakes with the real server and must read the colors from a full
surface and a delta. Nothing outside the repository is needed.

## Changes versus the 0.9.4 fork

The 0.9.4 fork (branch `fix/underline-color-wire-0.9.4`) applied to v0.9.5
without conflicts, and every commit's patch is unchanged (`git range-diff`
shows each one as identical).

- v0.9.5 changes no file under `src/protocol`, `src/server` or `src/pane`.
  In `src/client` it changes only input handling and its tests. So it adds no
  new way to send pane cells to a client.
- `PROTOCOL_VERSION` (22) and the endpoint generation (1) are unchanged, and
  v0.9.5 does not change the handoff code, so a 0.9.4-fork peer and a
  0.9.5-fork peer negotiate `shell.surface.v2` with each other.
- The build still needs Zig 0.16.0.

## Changes from the 0.9.3 fork to the 0.9.4 fork

The 0.9.3 fork (branch `fix/underline-color-wire-0.9.3`) applied to v0.9.4
without conflicts, and its code is unchanged here except for one line in a
test that is new in v0.9.4: that test builds the client-connect event by hand,
so it now names the surface codec (v1).

- v0.9.4 adds no new way to send pane cells to a client. The framing calls,
  the places that build cells and the two cell comparisons are the same as in
  the 0.9.3 fork. Upstream's edits to these files change cursor hiding,
  pixel mouse reports and the interactive render budget, not cells.
- `PROTOCOL_VERSION` (22) and the endpoint generation (1) are unchanged, so a
  0.9.3-fork peer and a 0.9.4-fork peer negotiate `shell.surface.v2` with each
  other.
- The build still needs Zig 0.16.0.

## Changes from the 0.9.0 fork to the 0.9.3 fork

The 0.9.0 fork (branch `fix/underline-color-wire-0.9.0`) has the same three
parts. Porting it to 0.9.3 changed these things:

- The v2 trailer now also rides on `surface_delta` and `surface_scroll`, which
  did not exist in 0.9.0. This branch's client turns both on, as stock 0.9.3
  does, so without that it would lose the color on every update sent that way.
- `CellData` gained a bincode `Decode` in 0.9.3 for the delta decoder. It is
  now written by hand so it reads exactly the fields the frozen layout has;
  a derived one would also read the new underline color field.
- 0.9.3 caches the last cell style while writing ANSI. The cache key now
  includes the underline color, so two neighbouring cells that differ only in
  underline color still get their own SGR.
- The negotiated codec lives in the client's render state on the server, next
  to the reuse, delta and scroll switches, instead of a separate field on the
  connection.
- The build needs Zig 0.16.0 instead of 0.15.2.

## History

The earlier patch for herdr 0.8.x (branch `fix/underline-color-wire`, commit
`f03d6209`) wrote SGR 58 in the semicolon form, `58;2;r;g;b`; the 0.9.0,
0.9.3, 0.9.4 and 0.9.5 series write the colon form.
