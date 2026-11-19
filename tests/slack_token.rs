//! Differential test for the Slack token matcher.
//!
//! The library matches tokens with hand-written code instead of Go shoutrrr's `tokenPattern`
//! regex. This runs both over generated inputs and checks that they normalize every one of them
//! identically, so the two can't drift apart unnoticed.

use regex::Regex;
use shoutrrr::services::slack::Token;

/// Go shoutrrr's `tokenPattern`, verbatim. If upstream changes it, change it here and the test
/// will show what the matcher in `src/services/slack/token.rs` needs to learn.
const TOKEN_PATTERN: &str = r"(?:(?P<type>xox.|hook)[-:]|:?)(?P<p1>[A-Z0-9]{9,})(?P<s1>[-/,])(?P<p2>[A-Z0-9]{9,})(?P<s2>[-/,])(?P<p3>[A-Za-z0-9]{24,})";

/// What setting a token from `input` leaves behind: whether it was accepted, and the stored value
/// (which is also written when the separators mismatch).
type Outcome = (bool, String);

fn actual(input: &str) -> Outcome {
    let mut token = Token::default();
    let accepted = token.set_from_prop(input).is_ok();
    (accepted, token.as_str().to_string())
}

/// The regex-based implementation the matcher replaced.
fn expected(pattern: &Regex, input: &str) -> Outcome {
    if input.len() < 3 {
        return (false, String::new());
    }
    let Some(caps) = pattern.captures(input) else {
        return (false, String::new());
    };
    let type_id = caps.name("type").map_or("hook", |m| m.as_str());
    let raw = format!("{type_id}:{}-{}-{}", &caps["p1"], &caps["p2"], &caps["p3"]);
    (caps["s1"] == caps["s2"], raw)
}

/// A small xorshift generator: deterministic, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[self.below(options.len())]
    }

    /// A string of `pick(lengths)` pieces drawn from `alphabet`.
    fn run(&mut self, alphabet: &[&str], lengths: &[usize]) -> String {
        let len = self.pick(lengths);
        (0..len).map(|_| self.pick(alphabet)).collect()
    }
}

const UPPER: &[&str] = &["A", "Z", "0", "9", "Q"];
const MIXED: &[&str] = &["A", "z", "0", "b", "Q", "x", "h"];
// Lengths on both sides of the 9- and 24-character minimums.
const PART_LENGTHS: &[usize] = &[0, 8, 9, 9, 10, 10, 12, 12];
const SECRET_LENGTHS: &[usize] = &[0, 23, 24, 24, 25, 25, 30, 30];
const SEPARATORS: &[&str] = &["-", "-", "-", "/", "/", ",", ",", ":", "", "_", "--"];
const PREFIXES: &[&str] = &[
    "",
    "",
    "xoxb",
    "xoxp",
    "hook",
    "xox",
    "xox\n",
    "xoxé",
    "xox-",
    "hoo",
    "HOOK",
    "xoxbb",
    ":",
    "::",
    "x",
    "junk ",
    "xoxb:xoxb",
    "hookhook",
    "AAAAAAAAA-",
    "é",
];
const TYPE_SEPARATORS: &[&str] = &["-", ":", "", "/", ",", "::", "-:"];
const NOISE: &[&str] = &[
    "",
    "",
    " ",
    "@webhook",
    "\n",
    "é",
    "-",
    ":",
    "xoxb-",
    "AAAAAAAAAA",
    "/",
];
const SOUP: &[&str] = &[
    "A",
    "B",
    "0",
    "a",
    "x",
    "o",
    "h",
    "k",
    "-",
    "/",
    ",",
    ":",
    "\n",
    "é",
    "xox",
    "hook",
    "AAAAAAAAA",
    "aaaaaaaaaaaaaaaaaaaaaaaa",
];

/// A token-shaped string with each piece independently perturbed.
fn token_like(rng: &mut Rng) -> String {
    let mut s = String::new();
    s += rng.pick(NOISE);
    s += rng.pick(PREFIXES);
    s += rng.pick(TYPE_SEPARATORS);
    s += &rng.run(UPPER, PART_LENGTHS);
    s += rng.pick(SEPARATORS);
    s += &rng.run(UPPER, PART_LENGTHS);
    s += rng.pick(SEPARATORS);
    s += &rng.run(MIXED, SECRET_LENGTHS);
    s += rng.pick(NOISE);
    if rng.below(4) == 0 {
        // A further part or two, so a failed match can be followed by one that starts later.
        s += rng.pick(SEPARATORS);
        s += &rng.run(UPPER, &[9, 10]);
        s += rng.pick(SEPARATORS);
        s += &rng.run(MIXED, &[24, 30]);
    }
    s
}

/// An unstructured mix of the characters and fragments the token grammar cares about.
fn soup(rng: &mut Rng) -> String {
    rng.run(SOUP, &[0, 1, 3, 8, 16, 32])
}

#[test]
fn matcher_agrees_with_go_token_pattern() {
    const INPUTS: usize = 100_000;

    let pattern = Regex::new(TOKEN_PATTERN).unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut matched = 0;

    for i in 0..INPUTS {
        let input = if i % 3 == 0 {
            soup(&mut rng)
        } else {
            token_like(&mut rng)
        };
        let want = expected(&pattern, &input);
        assert_eq!(actual(&input), want, "input: {input:?}");
        matched += usize::from(!want.1.is_empty());
    }

    // Guard the generator itself: the comparison means little unless it produces plenty of both
    // tokens and non-tokens.
    assert!(
        (INPUTS / 20..INPUTS / 2).contains(&matched),
        "{matched} of {INPUTS} inputs held a token"
    );
}

#[test]
fn matcher_agrees_on_notable_inputs() {
    let pattern = Regex::new(TOKEN_PATTERN).unwrap();
    for input in [
        "",
        "xoxb",
        "AAAAAAAAA/BBBBBBBBB/123456789123456789123456",
        "xoxb:AAAAAAAAA-BBBBBBBBB-123456789123456789123456",
        "hook-AAAAAAAAA,BBBBBBBBB,123456789123456789123456",
        ":AAAAAAAAA-BBBBBBBBB-123456789123456789123456",
        // Mismatched separators.
        "AAAAAAAAA-BBBBBBBBB/123456789123456789123456",
        // Text around the token is ignored.
        "see xoxp-AAAAAAAAA-BBBBBBBBB-123456789123456789123456 (prod)",
        // The first candidate fails on its third part; the match starts at the second part.
        "AAAAAAAAA-BBBBBBBBB-CCCCCCCCC-123456789123456789123456",
        // The type's fourth character is unconstrained, short of a newline.
        "xoxé:AAAAAAAAA-BBBBBBBBB-123456789123456789123456",
        "xox\n:AAAAAAAAA-BBBBBBBBB-123456789123456789123456",
        // Parts that are one character short.
        "AAAAAAAA-BBBBBBBBB-123456789123456789123456",
        "AAAAAAAAA-BBBBBBBBB-12345678912345678912345",
    ] {
        assert_eq!(actual(input), expected(&pattern, input), "input: {input:?}");
    }
}
