use super::*;
use crate::protocol::endpoint::SurfaceCodec;

const RED_UNDERCURL: &[u8] = b"\x1b[4:3m\x1b[58:2::255:0:0m";
const GREEN_UNDERCURL: &[u8] = b"\x1b[4:3m\x1b[58:2::0:255:0m";

fn red() -> u32 {
    protocol::color_to_u32(ratatui::style::Color::Rgb(255, 0, 0))
}

fn green() -> u32 {
    protocol::color_to_u32(ratatui::style::Color::Rgb(0, 255, 0))
}

fn colored(color: &[u8], text: &str) -> Vec<u8> {
    [color, text.as_bytes(), b"\x1b[0m"].concat()
}

/// A v2 client: the server writes v2 frames and its decoder reads them.
fn v2_client(
    server: &mut HeadlessServer,
    enable: impl FnOnce(&mut crate::server::render_stream::ClientRenderState),
) -> protocol::surface_reuse::Decoder {
    let state = &mut server.clients.get_mut(&1).expect("client").render_state;
    state.set_surface_codec(SurfaceCodec::V2);
    enable(state);
    protocol::surface_reuse::Decoder::new(true, true).with_surface_codec(SurfaceCodec::V2)
}

fn receive_v2(receiver: &std::sync::mpsc::Receiver<Vec<u8>>) -> ServerMessage {
    let bytes = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("render message");
    protocol::read_server_message(
        &mut std::io::Cursor::new(bytes),
        protocol::MAX_GRAPHICS_FRAME_SIZE,
        SurfaceCodec::V2,
    )
    .expect("decode v2 server message")
}

fn kind_of(message: &ServerMessage) -> &str {
    match message {
        ServerMessage::EndpointControl { kind, .. } => kind,
        ServerMessage::PaneSurface(_) => "pane-surface",
        ServerMessage::PaneSurfacePatch(_) => "pane-surface-patch",
        _ => "other",
    }
}

fn committed_cells(server: &HeadlessServer) -> Vec<protocol::CellData> {
    server.clients[&1]
        .render_state
        .last_pane_surface()
        .expect("committed surface")
        .frame
        .cells
        .clone()
}

fn color_of(cells: &[protocol::CellData], symbol: &str) -> u32 {
    cells
        .iter()
        .find(|cell| cell.symbol == symbol)
        .unwrap_or_else(|| panic!("no cell shows {symbol:?}"))
        .underline_color
}

fn apply_patch(frame: &mut FrameData, patch: &protocol::PaneSurfacePatch) {
    for row in &patch.rows {
        let start = usize::from(row.y) * usize::from(frame.width) + usize::from(row.x);
        frame.cells[start..start + row.cells.len()].clone_from_slice(&row.cells);
    }
}

fn initial_surface(
    server: &mut HeadlessServer,
    render_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    decoder: &mut protocol::surface_reuse::Decoder,
) -> protocol::PaneSurfaceFrame {
    server.render_and_stream();
    match decoder.decode(receive_v2(render_rx)).expect("initial") {
        ServerMessage::PaneSurface(surface) => surface,
        other => panic!("expected the initial pane surface, got {other:?}"),
    }
}

#[tokio::test]
async fn v2_full_surface_and_patch_carry_underline_color() {
    let (mut server, _control_rx, render_rx, pane_id) =
        retained_test_server_with_control(&colored(RED_UNDERCURL, "R"));
    let mut decoder = v2_client(&mut server, |_| {});
    let mut shell = initial_surface(&mut server, &render_rx, &mut decoder);
    assert_eq!(color_of(&shell.frame.cells, "R"), red());

    write_shared_test_pane(&mut server, pane_id, &colored(GREEN_UNDERCURL, "G"));
    assert!(server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
    let message = receive_v2(&render_rx);
    assert_eq!(kind_of(&message), "pane-surface-patch");
    let ServerMessage::PaneSurfacePatch(patch) = decoder.decode(message).expect("patch") else {
        panic!("expected a pane patch");
    };
    apply_patch(&mut shell.frame, &patch);
    assert_eq!(color_of(&shell.frame.cells, "G"), green());
    assert_eq!(shell.frame.cells, committed_cells(&server));
    shutdown_test_runtimes(&mut server);
}

#[tokio::test]
async fn v2_surface_delta_carries_underline_color_and_color_only_changes() {
    let (mut server, _control_rx, render_rx, pane_id) =
        retained_test_server_with_control(b"plain text");
    let mut decoder = v2_client(&mut server, |state| state.enable_surface_delta(true));
    let _ = initial_surface(&mut server, &render_rx, &mut decoder);

    for (color, expected) in [(RED_UNDERCURL, red()), (GREEN_UNDERCURL, green())] {
        // The second round changes only the underline color of the same text.
        write_shared_test_pane(
            &mut server,
            pane_id,
            &[b"\r", &*colored(color, "U")].concat(),
        );
        server.clients.get_mut(&1).unwrap().request_recompute();
        assert!(!server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
        server.render_and_stream();
        let message = receive_v2(&render_rx);
        assert_eq!(kind_of(&message), protocol::surface_delta::MESSAGE_KIND);
        let ServerMessage::PaneSurface(decoded) = decoder.decode(message).expect("delta") else {
            panic!("expected a reconstructed pane surface");
        };
        assert_eq!(color_of(&decoded.frame.cells, "U"), expected);
        assert_eq!(decoded.frame.cells, committed_cells(&server));
    }
    shutdown_test_runtimes(&mut server);
}

/// Build output with one colored line that scrolls up and colored new lines.
///
/// Styling every line makes the server send a plain patch instead of a scroll.
fn colored_scrolling_lines(range: std::ops::Range<usize>) -> Vec<u8> {
    range
        .flat_map(|n| {
            let color: &[u8] = match n {
                30 | 40 => RED_UNDERCURL,
                41 => GREEN_UNDERCURL,
                _ => b"",
            };
            [
                color,
                format!(
                    "build step {n:>3}: compiled module_{n} in {}ms\x1b[0m\r\n",
                    n * 7 % 97
                )
                .as_bytes(),
            ]
            .concat()
        })
        .collect()
}

#[tokio::test]
async fn v2_surface_scroll_carries_underline_color_on_new_and_shifted_rows() {
    let (mut server, _control_rx, render_rx, pane_id) =
        retained_test_server_with_control(&colored_scrolling_lines(0..40));
    let mut decoder = v2_client(&mut server, |state| state.enable_surface_scroll(true));
    let mut shell = initial_surface(&mut server, &render_rx, &mut decoder);

    write_shared_test_pane(&mut server, pane_id, &colored_scrolling_lines(40..42));
    assert!(server.render_retained_pane_surface_and_stream(&HashSet::from([pane_id])));
    let message = receive_v2(&render_rx);
    assert_eq!(kind_of(&message), protocol::surface_scroll::MESSAGE_KIND);
    let ServerMessage::PaneSurfacePatch(patch) = decoder.decode(message).expect("scroll") else {
        panic!("expected an expanded pane patch");
    };
    apply_patch(&mut shell.frame, &patch);
    let committed = committed_cells(&server);
    assert!(frame_text(&shell.frame).contains("module_41"));
    assert!(committed.iter().any(|cell| cell.underline_color == red()));
    assert!(committed.iter().any(|cell| cell.underline_color == green()));
    assert_eq!(shell.frame.cells, committed);
    shutdown_test_runtimes(&mut server);
}

#[tokio::test]
async fn client_shell_connect_keeps_the_negotiated_surface_codec() {
    let mut server = test_headless_server();
    for (client_id, codec) in [(81, SurfaceCodec::V2), (82, SurfaceCodec::V1)] {
        let (writer, _control_rx, _render_rx) = test_client_writer();
        server.handle_server_event(ServerEvent::ClientShellConnected {
            client_id,
            surface_cols: 80,
            surface_rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            pixel_mouse: false,
            direct_graphics: false,
            endpoint_keybindings: false,
            mouse_capture: false,
            surface_active: false,
            surface_reuse: false,
            surface_delta: false,
            surface_scroll: false,
            surface_codec: codec,
            writer,
        });
        assert_eq!(
            server.clients[&client_id].render_state.surface_codec(),
            codec
        );
    }
    shutdown_test_runtimes(&mut server);
}
