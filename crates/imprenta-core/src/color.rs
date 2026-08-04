//! sRGB colour with straight (non-premultiplied) alpha.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB colour, 8 bits per channel, with straight alpha.
///
/// 8-bit channels because that is what PDF, Tailwind and every design tool
/// speak. The paint layer converts to whatever the output backend wants —
/// keeping a colour space abstraction here is what makes PDF/A's ICC
/// requirement reachable later without touching every call site.
/// `Hash` alongside `Eq` because a colour is a key: the spreadsheet writer
/// interns fills so that a hundred thousand rows in one colour cost one entry,
/// and four bytes compared for equality can be hashed on the same terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// 255 is fully opaque.
    pub a: u8,
}

/// Written as hex, because that is what everyone who picks a colour writes.
///
/// `{"r":27,"g":58,"b":92,"a":255}` on every coloured thing in a document of
/// nine thousand pages is noise in the file and noise for whoever reads it.
/// Alpha is left off when there is none, which is nearly always.
impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Color { r, g, b, a } = *self;
        serializer.serialize_str(&if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        })
    }
}

/// Read from hex, or from the channels written out.
///
/// Both, because the hex form is what an author writes and the long form is
/// what a program holding four numbers already has. Neither should have to
/// convert to satisfy the other.
///
/// # Written out rather than `#[serde(untagged)]`
///
/// Untagged is the obvious way to accept two shapes and it **always buffers**:
/// serde reads the whole value into an intermediate tree, then tries each
/// variant against it. That is a map, two boxed values and a string for every
/// colour in a document — and a spreadsheet with a fill on every row is one
/// colour per row. Measured on ten thousand styled cells it was most of what
/// they cost.
///
/// `deserialize_any` asks the format what is there and handles it once, which
/// is what untagged is doing anyway, minus the tree.
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Written)
    }
}

struct Written;

impl<'de> serde::de::Visitor<'de> for Written {
    type Value = Color;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a colour, as \"#1b3a5c\" or as {\"r\":27,\"g\":58,\"b\":92,\"a\":255}")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Color, E> {
        Color::parse_hex(text).map_err(E::custom)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Color, A::Error> {
        let (mut r, mut g, mut b, mut a) = (None, None, None, None);
        while let Some(key) = map.next_key::<String>()? {
            let slot = match key.as_str() {
                "r" => &mut r,
                "g" => &mut g,
                "b" => &mut b,
                "a" => &mut a,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                    continue;
                }
            };
            *slot = Some(map.next_value::<u8>()?);
        }
        Ok(Color {
            r: r.ok_or_else(|| serde::de::Error::missing_field("r"))?,
            g: g.ok_or_else(|| serde::de::Error::missing_field("g"))?,
            b: b.ok_or_else(|| serde::de::Error::missing_field("b"))?,
            // Opaque unless said otherwise, which is what the hex form does too.
            a: a.unwrap_or(255),
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ColorError {
    #[error("{input:?} is not a valid hex colour (expected #rgb, #rgba, #rrggbb or #rrggbbaa)")]
    InvalidHex { input: String },
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// An opaque colour from 8-bit channels.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Parses CSS hex notation, with or without the leading `#`.
    pub fn parse_hex(input: &str) -> Result<Self, ColorError> {
        let invalid = || ColorError::InvalidHex {
            input: input.to_string(),
        };
        let hex = input.strip_prefix('#').unwrap_or(input);

        let nibbles: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()
            .ok_or_else(invalid)?;

        // Shorthand duplicates each nibble: #abc is #aabbcc, never #a0b0c0.
        let channels: Vec<u8> = match nibbles.len() {
            3 | 4 => nibbles.iter().map(|n| n * 17).collect(),
            6 | 8 => nibbles.chunks(2).map(|p| p[0] * 16 + p[1]).collect(),
            _ => return Err(invalid()),
        };

        Ok(Self {
            r: channels[0],
            g: channels[1],
            b: channels[2],
            a: channels.get(3).copied().unwrap_or(255),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_is_opaque_by_default() {
        assert_eq!(Color::rgb(31, 78, 121).a, 255);
    }

    #[test]
    fn parses_six_digit_hex() {
        // ContaPro's corporate blue, as it appears in their stylesheets.
        assert_eq!(
            Color::parse_hex("#1F4E79"),
            Ok(Color {
                r: 31,
                g: 78,
                b: 121,
                a: 255
            })
        );
    }

    #[test]
    fn hex_is_case_insensitive_and_hash_is_optional() {
        let expected = Color::parse_hex("#1F4E79").unwrap();
        assert_eq!(Color::parse_hex("#1f4e79"), Ok(expected));
        assert_eq!(Color::parse_hex("1F4E79"), Ok(expected));
    }

    #[test]
    fn expands_three_digit_shorthand_by_duplicating_each_nibble() {
        // #abc is #aabbcc, not #a0b0c0.
        assert_eq!(
            Color::parse_hex("#abc"),
            Ok(Color {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
                a: 255
            })
        );
    }

    #[test]
    fn parses_eight_digit_hex_with_alpha() {
        assert_eq!(
            Color::parse_hex("#1F4E7980"),
            Ok(Color {
                r: 31,
                g: 78,
                b: 121,
                a: 0x80
            })
        );
    }

    #[test]
    fn expands_four_digit_shorthand_with_alpha() {
        assert_eq!(
            Color::parse_hex("#abcd"),
            Ok(Color {
                r: 0xaa,
                g: 0xbb,
                b: 0xcc,
                a: 0xdd
            })
        );
    }

    #[test]
    fn rejects_invalid_hex_quoting_the_input() {
        // The error has to carry the offending value: it ends up in a build
        // diagnostic pointing at a line of someone's template.
        for bad in ["#12345", "#gggggg", "", "#", "rebeccapurple"] {
            let err = Color::parse_hex(bad).expect_err("{bad:?} must be rejected");
            assert_eq!(
                err,
                ColorError::InvalidHex {
                    input: bad.to_string()
                }
            );
            assert!(err.to_string().contains(bad) || bad.is_empty());
        }
    }

    // ── how a colour crosses the wire ───────────────────────────────────

    /// A JSON string literal, quotes and all.
    fn quoted(text: &str) -> String {
        format!("\"{text}\"")
    }

    #[test]
    fn a_colour_is_written_as_the_hex_a_designer_would_recognise() {
        let json = serde_json::to_string(&Color::rgb(0x1b, 0x3a, 0x5c)).unwrap();

        assert_eq!(json, quoted("#1b3a5c"));
    }

    #[test]
    fn alpha_appears_only_when_there_is_some() {
        // Four channels on every colour in a document of nine thousand pages
        // is noise in the JSON and noise for whoever reads it.
        let opaque = serde_json::to_string(&Color::rgb(0, 0, 0)).unwrap();
        let translucent = serde_json::to_string(&Color {
            r: 0,
            g: 0,
            b: 0,
            a: 128,
        })
        .unwrap();

        assert_eq!(opaque, quoted("#000000"));
        assert_eq!(translucent, quoted("#00000080"));
    }

    #[test]
    fn a_colour_is_read_from_hex() {
        let color: Color = serde_json::from_str(&quoted("#1b3a5c")).unwrap();

        assert_eq!(color, Color::rgb(0x1b, 0x3a, 0x5c));
    }

    #[test]
    fn every_hex_shorthand_is_accepted() {
        for (input, expected) in [
            ("#fff", Color::rgb(255, 255, 255)),
            ("#f00f", Color::rgb(255, 0, 0)),
            ("#1b3a5c", Color::rgb(0x1b, 0x3a, 0x5c)),
        ] {
            let color: Color = serde_json::from_str(&quoted(input)).unwrap();
            assert_eq!(color, expected, "{input}");
        }
    }

    #[test]
    fn the_channels_are_still_accepted_written_out() {
        // A producer holding the numbers should not have to format them into
        // a string first, and neither should anything already written.
        let json = "{\"r\":27,\"g\":58,\"b\":92,\"a\":255}";

        let color: Color = serde_json::from_str(json).unwrap();

        assert_eq!(color, Color::rgb(27, 58, 92));
    }

    #[test]
    fn a_colour_survives_the_round_trip() {
        for color in [
            Color::BLACK,
            Color::rgb(1, 2, 3),
            Color {
                r: 9,
                g: 8,
                b: 7,
                a: 6,
            },
        ] {
            let json = serde_json::to_string(&color).unwrap();
            let back: Color = serde_json::from_str(&json).unwrap();
            assert_eq!(back, color, "{json}");
        }
    }

    #[test]
    fn a_colour_that_is_not_one_says_what_it_should_have_been() {
        let refused = serde_json::from_str::<Color>(&quoted("navy")).unwrap_err();

        let message = refused.to_string();
        assert!(message.contains("navy"), "{message}");
        assert!(message.contains("#rrggbb"), "{message}");
    }
}
