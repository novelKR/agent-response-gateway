use agent_response_gateway::{adapters::sse::SseDecoder, ir::IrError};

fn decode(chunks: &[&[u8]]) -> Vec<(String, String)> {
    let mut decoder = SseDecoder::new(4096).unwrap();
    let mut events = Vec::new();
    for &chunk in chunks {
        let mut input = chunk;
        while let Some(event) = decoder.next_event(&mut input).unwrap() {
            events.push((event.event, event.data));
        }
        assert!(input.is_empty());
    }
    decoder.finish().unwrap();
    events
}

#[test]
fn every_split_preserves_utf8_crlf_comments_and_multiline_data() {
    let wire="\u{feff}: heartbeat\r\nevent: synthetic\r\ndata: {\"text\":\"한글🧪\",\r\ndata: \"value\":3}\r\n\r\nid: ignored\nretry: 1\nevent:\ndata: second\n\n".as_bytes();
    let expected = vec![
        (
            "synthetic".into(),
            "{\"text\":\"한글🧪\",\n\"value\":3}".into(),
        ),
        ("message".into(), "second".into()),
    ];
    for split in 0..=wire.len() {
        assert_eq!(decode(&[&wire[..split], &wire[split..]]), expected);
    }
    assert_eq!(decode(&wire.chunks(1).collect::<Vec<_>>()), expected);
    assert_eq!(
        decode(&[b"data:a\r\rdata:b\r\r"]),
        vec![
            ("message".into(), "a".into()),
            ("message".into(), "b".into())
        ]
    );
}

#[test]
fn framing_rejects_invalid_utf8_unterminated_and_oversized_events() {
    let mut decoder = SseDecoder::new(32).unwrap();
    let mut invalid = b"data: \xff\n\n".as_slice();
    assert!(decoder.next_event(&mut invalid).is_err());
    let mut decoder = SseDecoder::new(32).unwrap();
    let mut partial = b"data: partial\n".as_slice();
    assert!(decoder.next_event(&mut partial).unwrap().is_none());
    assert!(decoder.finish().is_err());
    let mut decoder = SseDecoder::new(16).unwrap();
    let mut large = b"data: 01234567890123456789\n\n".as_slice();
    assert!(matches!(
        decoder.next_event(&mut large),
        Err(IrError::SizeLimit)
    ));
}

#[test]
fn parser_returns_one_event_before_consuming_remainder() {
    let mut decoder = SseDecoder::new(32).unwrap();
    let mut input = b"data: first\n\ndata: second\n\n".as_slice();
    assert_eq!(
        decoder.next_event(&mut input).unwrap().unwrap().data,
        "first"
    );
    assert_eq!(input, b"data: second\n\n");
    assert_eq!(
        decoder.next_event(&mut input).unwrap().unwrap().data,
        "second"
    );
    decoder.finish().unwrap();
}
