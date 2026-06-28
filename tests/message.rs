//! Tests for the message partition/chunk helpers ported from Go's `pkg/util/message.go`.
//! These pin the subtle ported arithmetic (truncation, the never-reset total-length cascade,
//! the backward whitespace search, and rune-correct chunking) that no service test exercises
//! directly.

use shoutrrr::message::{MessageItem, MessageLimit, message_items_from_lines, partition_message};

fn texts(items: &[MessageItem]) -> Vec<&str> {
    items.iter().map(|i| i.text.as_str()).collect()
}

#[test]
fn long_line_is_truncated_with_ellipsis() {
    // Mirrors Go's TestMessageItemsFromLines_Truncation: a line over chunk_size is cut to
    // `chunk_size` runes total (keep = chunk_size - len(ellipsis), then " [...]" appended).
    let limits = MessageLimit {
        chunk_size: 20,
        total_chunk_size: 100,
        chunk_count: 5,
    };
    let batches = message_items_from_lines(&"A".repeat(50), limits);

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    let text = &batches[0][0].text;
    assert_eq!(text.chars().count(), 20);
    assert!(text.ends_with(" [...]"));
}

#[test]
fn total_length_is_not_reset_per_batch() {
    // Faithful to Go: `totalLength` accumulates across batches and is never reset, so once
    // `total_length + chunk_size` exceeds total_chunk_size every later line opens its own batch.
    // chunk_size=10, total=25, 7 lines of 5 runes each => [4 lines][1][1][1].
    let limits = MessageLimit {
        chunk_size: 10,
        total_chunk_size: 25,
        chunk_count: 100,
    };
    let input = ["aaaaa"; 7].join("\n");
    let batches = message_items_from_lines(&input, limits);

    let sizes: Vec<usize> = batches.iter().map(Vec::len).collect();
    assert_eq!(sizes, vec![4, 1, 1, 1]);
}

#[test]
fn empty_lines_are_skipped() {
    let limits = MessageLimit {
        chunk_size: 10,
        total_chunk_size: 100,
        chunk_count: 10,
    };
    let batches = message_items_from_lines("a\n\nb", limits);

    assert_eq!(batches.len(), 1);
    assert_eq!(texts(&batches[0]), vec!["a", "b"]);
}

#[test]
fn tiny_chunk_size_does_not_panic() {
    // chunk_size < len(" [...]") would underflow `keep` without the saturating guard; the
    // resulting zero-length item is then skipped. Go documents chunk_size >= len(ellipsis) and
    // panics otherwise — this port stays total.
    let limits = MessageLimit {
        chunk_size: 3,
        total_chunk_size: 100,
        chunk_count: 5,
    };
    let batches = message_items_from_lines("abcdefgh", limits);
    assert!(batches.is_empty());
}

#[test]
fn partition_breaks_at_whitespace() {
    let limits = MessageLimit {
        chunk_size: 8,
        total_chunk_size: 100,
        chunk_count: 5,
    };
    let (items, omitted) = partition_message("hello world", limits, 5);

    assert_eq!(texts(&items), vec!["hello", "world"]);
    assert_eq!(omitted, 0);
}

#[test]
fn partition_small_chunk_large_distance_does_not_panic() {
    // The backward search reaches index 0; the Rust guard avoids the usize underflow that panics
    // in Go (`runes[-1]`). No whitespace exists, so chunks are exact chunk_size slices.
    let limits = MessageLimit {
        chunk_size: 2,
        total_chunk_size: 100,
        chunk_count: 5,
    };
    let (items, omitted) = partition_message("ABCDEFGH", limits, 10);

    assert_eq!(texts(&items), vec!["AB", "CD", "EF", "GH"]);
    assert_eq!(omitted, 0);
}

#[test]
fn partition_reports_omitted_runes() {
    // chunk_count - 1 = 1 usable chunk of size 4; the rest is omitted.
    let limits = MessageLimit {
        chunk_size: 4,
        total_chunk_size: 100,
        chunk_count: 2,
    };
    let (items, omitted) = partition_message("ABCDEFGHIJ", limits, 0);

    assert_eq!(texts(&items), vec!["ABCD"]);
    assert_eq!(omitted, 6);
}

#[test]
fn partition_empty_input() {
    let limits = MessageLimit {
        chunk_size: 10,
        total_chunk_size: 100,
        chunk_count: 5,
    };
    let (items, omitted) = partition_message("", limits, 5);

    assert!(items.is_empty());
    assert_eq!(omitted, 0);
}
