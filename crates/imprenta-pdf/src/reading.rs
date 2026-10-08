//! Reading the words back out of a file this engine wrote, for tests.
//!
//! A page number, a carried total and a table of contents are all words that
//! change from page to page, and counting drawing operators cannot tell "1 de
//! 3" from "3 de 1". This reads what each page says, the way a reader copying
//! text out of it would: the content stream's glyphs, through the `ToUnicode`
//! map of the font each was drawn in.
//!
//! Only for files written with compression off, and only for the syntax the
//! writer in this repository produces. It is not a PDF parser and must not
//! grow into one; anything it cannot read it leaves out.

use std::collections::HashMap;

/// What each page says, one string per text run, in the order drawn.
pub(crate) fn page_texts(pdf: &[u8]) -> Vec<Vec<String>> {
    let objects = objects(pdf);
    let tree = objects
        .values()
        .find(|body| text(body).contains("/Kids ["))
        .expect("no page tree");
    let tree = text(tree);
    let kids = between(&tree, "/Kids [", "]").expect("no kids");

    references(kids)
        .into_iter()
        .map(|page| {
            let page = text(objects[&page]);
            let contents = references(between(&page, "/Contents", "\n").unwrap_or_default());
            let fonts: HashMap<String, HashMap<u16, String>> = between(&page, "/Font <<", ">>")
                .unwrap_or_default()
                .lines()
                .filter_map(|line| {
                    let (name, reference) = line.trim().split_once(' ')?;
                    let font = text(objects[references(reference).first()?]);
                    let map = references(between(&font, "/ToUnicode", "\n")?)
                        .first()
                        .map(|to_unicode| cmap(objects[to_unicode]))?;
                    Some((name.trim_start_matches('/').to_string(), map))
                })
                .collect();
            contents
                .first()
                .map(|stream| runs(objects[stream], &fonts))
                .unwrap_or_default()
        })
        .collect()
}

/// Every indirect object's body, by number.
fn objects(pdf: &[u8]) -> HashMap<usize, &[u8]> {
    let mut out = HashMap::new();
    let mut at = 0;
    while let Some(found) = find(&pdf[at..], b" 0 obj\n") {
        let head = at + found;
        let start = pdf[..head]
            .iter()
            .rposition(|b| !b.is_ascii_digit())
            .map_or(0, |i| i + 1);
        let number: usize = text(&pdf[start..head]).parse().unwrap_or(0);
        let body = head + b" 0 obj\n".len();
        let end = find(&pdf[body..], b"endobj").map_or(pdf.len(), |e| body + e);
        out.insert(number, &pdf[body..end]);
        at = end;
    }
    out
}

/// The text runs a content stream draws.
fn runs(stream: &[u8], fonts: &HashMap<String, HashMap<u16, String>>) -> Vec<String> {
    let Some(start) = find(stream, b"stream\n") else {
        return Vec::new();
    };
    let body = &stream[start + 7..];
    let mut out = Vec::new();
    let mut font = None;
    let mut i = 0;
    while i < body.len() {
        match body[i] {
            b'/' if body[i + 1..].starts_with(b"f") => {
                let end = body[i..]
                    .iter()
                    .position(|b| *b == b' ')
                    .map_or(body.len(), |e| i + e);
                font = fonts.get(&text(&body[i + 1..end]));
                i = end;
            }
            b'[' => {
                let mut run = String::new();
                i += 1;
                while i < body.len() && body[i] != b']' {
                    if body[i] == b'(' {
                        let (bytes, next) = literal(body, i + 1);
                        for pair in bytes.chunks(2) {
                            let cid = u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]);
                            if let Some(chars) = font.and_then(|f| f.get(&cid)) {
                                run.push_str(chars);
                            }
                        }
                        i = next;
                    } else {
                        i += 1;
                    }
                }
                out.push(run);
            }
            _ => i += 1,
        }
    }
    out
}

/// A literal string's bytes, from just inside its opening parenthesis, and
/// where reading should carry on.
fn literal(body: &[u8], mut i: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 0;
    while i < body.len() {
        match body[i] {
            b'\\' => {
                let next = body[i + 1];
                if next.is_ascii_digit() {
                    let digits: Vec<u8> = body[i + 1..]
                        .iter()
                        .take(3)
                        .take_while(|b| (b'0'..=b'7').contains(b))
                        .copied()
                        .collect();
                    out.push(u8::from_str_radix(&text(&digits), 8).unwrap_or(0));
                    i += 1 + digits.len();
                    continue;
                }
                out.push(match next {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 8,
                    b'f' => 12,
                    other => other,
                });
                i += 2;
                continue;
            }
            b'(' => depth += 1,
            b')' if depth == 0 => return (out, i + 1),
            b')' => depth -= 1,
            _ => {}
        }
        out.push(body[i]);
        i += 1;
    }
    (out, i)
}

/// A `ToUnicode` stream's single-character entries.
fn cmap(stream: &[u8]) -> HashMap<u16, String> {
    let stream = text(stream);
    let mut out = HashMap::new();
    for block in stream.split("beginbfchar").skip(1) {
        for line in block.split("endbfchar").next().unwrap_or("").lines() {
            let hex: Vec<&str> = line
                .split(['<', '>'])
                .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit()))
                .collect();
            let [cid, chars] = hex[..] else {
                continue;
            };
            let units: Vec<u16> = chars
                .as_bytes()
                .chunks(4)
                .filter_map(|c| u16::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
                .collect();
            if let (Ok(cid), Ok(chars)) = (u16::from_str_radix(cid, 16), String::from_utf16(&units))
            {
                out.insert(cid, chars);
            }
        }
    }
    out
}

/// The object numbers a run of `N 0 R` references names.
fn references(s: &str) -> Vec<usize> {
    let words: Vec<&str> = s.split_whitespace().collect();
    words
        .windows(3)
        .filter(|w| w[2].starts_with('R'))
        .filter_map(|w| w[0].trim_start_matches('[').parse().ok())
        .collect()
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = start + s[start..].find(close)?;
    Some(&s[start..end])
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{Assets, build};
    use crate::ir;
    use crate::render::Options;
    use crate::shape::Face;

    #[test]
    fn what_a_page_says_is_read_back_run_by_run_and_page_by_page() {
        let assets = Assets::new()
            .with_font(
                Face::REGULAR,
                include_bytes!("../tests/fonts/Roboto-Regular.ttf").to_vec(),
            )
            .with_font(
                Face::BOLD,
                include_bytes!("../tests/fonts/Roboto-Bold.ttf").to_vec(),
            );
        let document: ir::Document = serde_json::from_str(
            r#"{ "children": [
                { "t": "text", "runs": [{ "text": "Hola " }, { "text": "mundo", "weight": "bold" }] },
                { "t": "pageBreak" },
                { "t": "text", "runs": [{ "text": "Adiós (y gracias)" }] }
            ]}"#,
        )
        .unwrap();

        let built = build(&document, &assets, Options { compress: false }).unwrap();

        assert_eq!(
            page_texts(&built.pdf),
            vec![
                vec!["Hola ".to_string(), "mundo".to_string()],
                vec!["Adiós (y gracias)".to_string()],
            ]
        );
    }

    #[test]
    fn a_ligature_copies_out_as_every_letter_it_stands_for() {
        // Roboto sets "fi" as one glyph. Drawn, it is right; copied, it was an
        // "f" alone, so "fin" came out of the file as "fn" and a search for
        // "fiscal" found nothing — on every page, with nothing to say so.
        let assets = Assets::new().with_font(
            Face::REGULAR,
            include_bytes!("../tests/fonts/Roboto-Regular.ttf").to_vec(),
        );
        let document: ir::Document = serde_json::from_str(
            r#"{ "children": [{ "t": "text", "runs": [{ "text": "Fin del ejercicio fiscal" }] }] }"#,
        )
        .unwrap();

        let built = build(&document, &assets, Options { compress: false }).unwrap();

        assert_eq!(
            page_texts(&built.pdf),
            vec![vec!["Fin del ejercicio fiscal".to_string()]]
        );
    }
}
