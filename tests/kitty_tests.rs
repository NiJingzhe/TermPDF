use termpdf::kitty::{
    encode_delete_image, encode_positioned_put_existing_image, encode_probe_query,
    encode_put_existing_image, encode_transmit_and_display, encode_transmit_only,
    parse_probe_response, wrap_command_for_transport, HighlightRendererState, KittyImageIds,
    KittyProbeResult, KittyTransport, RendererState,
};
use termpdf::render::RenderedPage;

fn rendered_page(bytes: Vec<u8>) -> RenderedPage {
    RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 1,
        bitmap_height: 1,
        crop_x: 0,
        crop_y: 0,
        crop_width: 1,
        crop_height: 1,
        placement_columns: 2,
        placement_rows: 3,
        rgba: bytes,
    }
}

#[test]
fn encodes_single_chunk_rgba_transfer() {
    let commands = encode_transmit_and_display(
        &rendered_page(vec![0x00, 0x00, 0x00, 0xff]),
        KittyImageIds {
            image_id: 9,
            placement_id: 4,
        },
    );

    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("\x1b_Ga=T,q=2,i=9,p=4,f=32,s=1,v=1,o=z,C=1,c=2,r=3,z=-1,m=0;"));
    assert!(commands[0].ends_with("\x1b\\"));
}

#[test]
fn encodes_chunked_transfer_with_follow_up_chunks() {
    let bytes = (0..400_000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let commands = encode_transmit_and_display(&rendered_page(bytes), KittyImageIds::DEFAULT);

    assert!(commands.len() > 1);
    assert!(commands[0].contains(",o=z,"));
    assert!(commands[0].contains(",m=1;"));
    assert!(commands[0].starts_with("\x1b_Ga=T,q=2,i=1,p=1,f=32,s=1,v=1,o=z,C=1,c=2,r=3,z=-1"));
    assert!(commands[1].starts_with("\x1b_Gm="));
    assert!(commands
        .iter()
        .skip(1)
        .all(|command| command.contains(",q=2;")));
    assert!(commands.last().unwrap().starts_with("\x1b_Gm=0,q=2;"));
}

#[test]
fn encodes_image_delete_command() {
    let command = encode_delete_image(KittyImageIds {
        image_id: 12,
        placement_id: 7,
    });

    assert_eq!(command, "\x1b_Ga=d,d=I,q=2,i=12\x1b\\");
}

#[test]
fn encode_transmit_and_display_does_not_embed_crop_parameters() {
    let commands = encode_transmit_and_display(
        &RenderedPage {
            page_index: 0,
            placement_col: 0,
            placement_row: 0,
            bitmap_width: 600,
            bitmap_height: 800,
            crop_x: 0,
            crop_y: 0,
            crop_width: 300,
            crop_height: 400,
            placement_columns: 38,
            placement_rows: 20,
            rgba: vec![0u8; 4],
        },
        KittyImageIds::DEFAULT,
    );

    assert!(!commands[0].contains(",x="));
    assert!(!commands[0].contains(",y="));
    assert!(!commands[0].contains(",w="));
    assert!(!commands[0].contains(",h="));
}

#[test]
fn encodes_transmit_only_without_initial_placement() {
    let commands = encode_transmit_only(&rendered_page(vec![0x00, 0x00, 0x00, 0xff]), 21);

    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("\x1b_Ga=t,q=2,i=21,f=32,s=1,v=1,o=z,m=0;"));
    assert!(commands[0].ends_with("\x1b\\"));
}

#[test]
fn encodes_put_existing_image_with_simple_placement() {
    let command = encode_put_existing_image(
        &RenderedPage {
            page_index: 0,
            placement_col: 0,
            placement_row: 0,
            bitmap_width: 600,
            bitmap_height: 800,
            crop_x: 12,
            crop_y: 34,
            crop_width: 300,
            crop_height: 400,
            placement_columns: 38,
            placement_rows: 20,
            rgba: vec![],
        },
        KittyImageIds {
            image_id: 21,
            placement_id: 2,
        },
        -100,
    );

    assert_eq!(command, "\x1b_Ga=p,q=2,i=21,p=2,c=38,r=20,C=1,z=-100\x1b\\");
}

#[test]
fn renderer_state_reuses_slot_and_emits_no_commands_for_identical_frame() {
    let rendered = RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 600,
        bitmap_height: 800,
        crop_x: 0,
        crop_y: 0,
        crop_width: 300,
        crop_height: 400,
        placement_columns: 38,
        placement_rows: 20,
        rgba: vec![0u8; 4],
    };
    let mut state = RendererState::default();

    let first = state.prepare_commands(std::slice::from_ref(&rendered));
    let second = state.prepare_commands(&[RenderedPage {
        crop_x: rendered.crop_x,
        ..rendered.clone()
    }]);

    assert!(first[0].starts_with("\x1b_Ga=t,q=2,i=1,f=32,s=600,v=800,o=z"));
    assert!(first[1].starts_with("\x1b[1;1H\x1b_Ga=p,q=2,i=1,p=1,c=38,r=20,C=1,z="));
    assert!(second.is_empty());
}

#[test]
fn renderer_state_transmits_new_bitmap_when_size_changes() {
    let mut state = RendererState::default();

    let first = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 600,
        bitmap_height: 800,
        crop_x: 0,
        crop_y: 0,
        crop_width: 300,
        crop_height: 400,
        placement_columns: 38,
        placement_rows: 20,
        rgba: vec![0u8; 4],
    }]);
    let second = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 900,
        bitmap_height: 1200,
        crop_x: 0,
        crop_y: 0,
        crop_width: 300,
        crop_height: 400,
        placement_columns: 38,
        placement_rows: 20,
        rgba: vec![0u8; 4],
    }]);

    assert!(first[0].starts_with("\x1b_Ga=t,q=2,i=1,f=32,s=600,v=800,o=z"));
    assert!(second[0].starts_with("\x1b_Ga=t,q=2,i=2,f=32,s=900,v=1200,o=z"));
    assert!(second[1].starts_with("\x1b[1;1H\x1b_Ga=p,q=2,i=2,p=1,c=38,r=20,C=1,z="));
    assert_eq!(second.len(), 3);
    assert_eq!(second[2], "\x1b_Ga=d,d=I,q=2,i=1\x1b\\");
}

#[test]
fn renderer_state_retransmits_when_placement_changes() {
    let mut state = RendererState::default();

    let first = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![0u8; 100 * 100 * 4],
    }]);
    let second = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 2,
        placement_row: 3,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![0u8; 100 * 100 * 4],
    }]);

    assert!(first[0].starts_with("\x1b_Ga=t,q=2,i=1,f=32,s=100,v=100,o=z"));
    assert!(second[0].starts_with("\x1b_Ga=t,q=2,i=2,f=32,s=100,v=100,o=z"));
    assert!(second[1].starts_with("\x1b[4;3H\x1b_Ga=p,q=2,i=2,p=1,c=10,r=10,C=1,z="));
    assert_eq!(second[2], "\x1b_Ga=d,d=I,q=2,i=1\x1b\\");
}

#[test]
fn highlight_renderer_reuses_same_mask_bitmap_across_crops() {
    let mut state = HighlightRendererState::default();

    let first = state.prepare_commands(&RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 24,
        bitmap_height: 10,
        crop_x: 0,
        crop_y: 0,
        crop_width: 24,
        crop_height: 10,
        placement_columns: 3,
        placement_rows: 1,
        rgba: vec![0u8; 24 * 10 * 4],
    });
    let second = state.prepare_commands(&RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 24,
        bitmap_height: 10,
        crop_x: 0,
        crop_y: 0,
        crop_width: 24,
        crop_height: 10,
        placement_columns: 3,
        placement_rows: 1,
        rgba: vec![0u8; 24 * 10 * 4],
    });

    assert!(first[0].starts_with("\x1b_Ga=t,q=2,i=10000,f=32,s=24,v=10,o=z"));
    assert!(second.is_empty());
}

#[test]
fn renderer_state_retransmits_same_size_bitmap_when_pixels_change() {
    let mut state = RendererState::default();

    let first = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![0u8; 100 * 100 * 4],
    }]);
    let second = state.prepare_commands(&[RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![255u8; 100 * 100 * 4],
    }]);

    assert!(first[0].starts_with("\x1b_Ga=t,q=2,i=1,f=32,s=100,v=100,o=z"));
    assert!(second[0].starts_with("\x1b_Ga=t,q=2,i=2,f=32,s=100,v=100,o=z"));
}

#[test]
fn renderer_state_deletes_pages_that_leave_visible_set() {
    let mut state = RendererState::default();
    let first = RenderedPage {
        page_index: 0,
        placement_col: 0,
        placement_row: 0,
        bitmap_width: 100,
        bitmap_height: 100,
        crop_x: 0,
        crop_y: 0,
        crop_width: 100,
        crop_height: 100,
        placement_columns: 10,
        placement_rows: 10,
        rgba: vec![0u8; 100 * 100 * 4],
    };
    let second = RenderedPage {
        page_index: 1,
        ..first.clone()
    };

    let first_commands = state.prepare_commands(&[first, second]);
    let second_commands = state.prepare_commands(&[]);

    assert_eq!(first_commands.len(), 4);
    assert_eq!(second_commands.len(), 2);
    assert!(second_commands
        .iter()
        .all(|command| command.starts_with("\x1b_Ga=d,d=I,q=2,i=")));
}

#[test]
fn encodes_positioned_put_with_cursor_move_prefix() {
    let command = encode_positioned_put_existing_image(
        &RenderedPage {
            page_index: 0,
            placement_col: 10,
            placement_row: 4,
            bitmap_width: 600,
            bitmap_height: 800,
            crop_x: 12,
            crop_y: 34,
            crop_width: 300,
            crop_height: 400,
            placement_columns: 38,
            placement_rows: 20,
            rgba: vec![],
        },
        KittyImageIds {
            image_id: 21,
            placement_id: 2,
        },
        -42,
    );

    assert!(command.starts_with("\x1b[5;11H\x1b_Ga=p"));
    assert!(command.contains(",z=-42"));
}

#[test]
fn wraps_commands_for_tmux_passthrough() {
    let wrapped =
        wrap_command_for_transport("\x1b_Ga=q;test\x1b\\", KittyTransport::TmuxPassthrough);

    assert_eq!(wrapped, "\x1bPtmux;\x1b\x1b_Ga=q;test\x1b\x1b\\\x1b\\");
}

#[test]
fn probe_query_encodes_query_action() {
    let query = encode_probe_query(77);

    assert_eq!(query, "\x1b_Ga=q,t=d,s=1,v=1,i=77;AAAA\x1b\\");
}

#[test]
fn parses_supported_probe_response() {
    let response = "\x1b_Gi=77;OK\x1b\\";

    assert_eq!(
        parse_probe_response(response, 77),
        KittyProbeResult::Supported
    );
}

#[test]
fn treats_any_query_response_with_matching_id_as_supported() {
    let response = "\x1b_Gi=77;ENOENT\x1b\\";

    assert_eq!(
        parse_probe_response(response, 77),
        KittyProbeResult::Supported
    );
}

#[test]
fn parses_unsupported_when_only_device_attributes_arrive() {
    let response = "\x1b[?62;c";

    assert_eq!(
        parse_probe_response(response, 77),
        KittyProbeResult::Unsupported
    );
}
