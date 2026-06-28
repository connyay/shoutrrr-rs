//! Message items and chunk/partition helpers.
//!
//! Ported from Go shoutrrr's `pkg/util/message.go` and `pkg/types` message primitives. Services
//! (Discord especially) use them to split long messages into API-sized pieces.

/// Suffix appended to truncated lines.
const ELLIPSIS: &str = " [...]";

/// Severity level attached to a [`MessageItem`].
///
/// The discriminant order matters: it indexes the per-level color array (see Discord's
/// `LevelColors`). `Unknown` is the default/plain level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageLevel {
    /// No specific level (plain message).
    #[default]
    Unknown = 0,
    /// Error.
    Error = 1,
    /// Warning.
    Warning = 2,
    /// Informational.
    Info = 3,
    /// Debug.
    Debug = 4,
}

/// Number of distinct [`MessageLevel`] variants (size of a per-level color array).
pub const MESSAGE_LEVEL_COUNT: usize = 5;

impl MessageLevel {
    /// Human-readable name, used e.g. as a Discord embed footer.
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageLevel::Unknown => "Unknown",
            MessageLevel::Error => "Error",
            MessageLevel::Warning => "Warning",
            MessageLevel::Info => "Info",
            MessageLevel::Debug => "Debug",
        }
    }
}

/// A single entry in a notification.
///
/// Only `text` and `level` are modeled in this bootstrap; timestamps, structured fields, and file
/// attachments from the Go original are deferred.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageItem {
    /// The item's text.
    pub text: String,
    /// The item's severity level.
    pub level: MessageLevel,
}

impl MessageItem {
    /// Construct a plain (`Unknown` level) item from text.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: MessageLevel::Unknown,
        }
    }
}

/// Size limits used when partitioning a message into chunks.
#[derive(Debug, Clone, Copy)]
pub struct MessageLimit {
    /// Maximum number of runes in a single chunk.
    pub chunk_size: usize,
    /// Maximum number of runes across all chunks.
    pub total_chunk_size: usize,
    /// Maximum number of chunks.
    pub chunk_count: usize,
}

/// Split `input` into chunks within `limits`, preferring to break at whitespace.
///
/// Searches backwards up to `distance` runes from each chunk boundary for a space or newline.
/// Returns the chunk items and the number of trailing runes that were omitted (exceeded limits).
/// Ports Go's `util.PartitionMessage`.
pub fn partition_message(
    input: &str,
    limits: MessageLimit,
    distance: usize,
) -> (Vec<MessageItem>, usize) {
    let mut items = Vec::new();
    if input.is_empty() {
        return (items, 0);
    }

    let runes: Vec<char> = input.chars().collect();
    let mut chunk_offset = 0usize;
    let max_total = runes.len().min(limits.total_chunk_size);
    let max_count = limits.chunk_count.saturating_sub(1);

    for _ in 0..max_count {
        let mut chunk_end = chunk_offset + limits.chunk_size;
        let mut next_chunk_start = chunk_end;

        if chunk_end >= max_total {
            chunk_end = max_total;
            next_chunk_start = max_total;
        } else {
            // Search backwards for a whitespace to split at a word boundary.
            for r in 0..distance {
                if r > chunk_end {
                    break;
                }
                let rp = chunk_end - r;
                if runes[rp] == '\n' || runes[rp] == ' ' {
                    chunk_end = rp;
                    next_chunk_start = chunk_end + 1;
                    break;
                }
            }
        }

        items.push(MessageItem::plain(
            runes[chunk_offset..chunk_end].iter().collect::<String>(),
        ));

        chunk_offset = next_chunk_start;
        if chunk_offset >= max_total {
            break;
        }
    }

    (items, runes.len() - chunk_offset)
}

/// Split `plain` by newlines into batches of items respecting `limits`.
///
/// Lines longer than `chunk_size` are truncated with an ellipsis; empty lines are skipped. Ports
/// Go's `util.MessageItemsFromLines`.
pub fn message_items_from_lines(plain: &str, limits: MessageLimit) -> Vec<Vec<MessageItem>> {
    let max_count = limits.chunk_count;
    let mut batches: Vec<Vec<MessageItem>> = Vec::new();
    let mut items: Vec<MessageItem> = Vec::new();
    let mut total_length = 0usize;

    for line in plain.split('\n') {
        let max_len = limits.chunk_size;

        if items.len() == max_count || total_length + max_len > limits.total_chunk_size {
            batches.push(std::mem::take(&mut items));
        }

        let rune_count = line.chars().count();
        let (text, rune_len) = if rune_count > max_len {
            // `saturating_sub` guards the degenerate `chunk_size < ELLIPSIS.len()` case: `keep`
            // becomes 0, the truncated text is just the ellipsis, and the `rune_len < 1` check
            // below skips it rather than underflowing. (Go's `maxLen-len(ellipsis)` panics here;
            // it documents `chunk_size >= len(ellipsis)` as a precondition.)
            let keep = max_len.saturating_sub(ELLIPSIS.len());
            let truncated: String = line.chars().take(keep).collect();
            (format!("{truncated}{ELLIPSIS}"), keep)
        } else {
            (line.to_string(), rune_count)
        };

        if rune_len < 1 {
            continue;
        }

        items.push(MessageItem::plain(text));
        total_length += rune_len;
    }

    if !items.is_empty() {
        batches.push(items);
    }

    batches
}
