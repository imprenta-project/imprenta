//! Writing XML by hand, because the XML we write is small and entirely known.
//!
//! There is no document to parse and no schema to discover: every element
//! this crate emits is one of a dozen shapes decided here. A generic writer
//! would cost a borrow check and a state machine per cell, and a spreadsheet
//! is ten million cells.
//!
//! What a writer must not get wrong is escaping, so that is the part with
//! tests. An unescaped ampersand in a concept line does not produce a wrong
//! cell — it produces a file Excel refuses to open, behind a repair dialog
//! that names nothing.

/// Appends `text` as XML character data.
///
/// `>` is escaped along with `<` and `&`, which the specification only
/// requires inside `]]>`. Escaping it always is one branch rather than a
/// look-behind, and no reader minds.
pub fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Tab, newline and carriage return are the only control
            // characters XML 1.0 allows. The rest are not merely discouraged;
            // a document containing one is not XML at all, and Excel writes
            // them as `_xHHHH_` rather than dropping the character. We follow,
            // so text survives a round trip through a spreadsheet.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {
                out.push_str(&format!("_x{:04X}_", c as u32));
            }
            c => out.push(c),
        }
    }
}

/// Escapes `text` into a new string. For the rare call that is not in a loop.
pub fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape(text, &mut out);
    out
}

/// Appends an Excel column name for a zero-based index: 0 is `A`, 26 is `AA`.
///
/// Bijective base-26, which is not ordinary base-26: there is no zero digit,
/// so `Z` is followed by `AA` rather than by `BA`.
pub fn column_name(index: u32, out: &mut String) {
    // Excel stops at 16,384 columns, which is XFD — three letters, always.
    let mut buf = [0u8; 3];
    let mut at = buf.len();
    let mut n = index + 1;
    while n > 0 {
        at -= 1;
        buf[at] = b'A' + ((n - 1) % 26) as u8;
        n = (n - 1) / 26;
    }
    out.push_str(std::str::from_utf8(&buf[at..]).expect("ASCII letters are UTF-8"));
}

/// Appends a cell reference such as `C7`, from zero-based row and column.
pub fn cell_ref(row: u32, column: u32, out: &mut String) {
    column_name(column, out);
    // `itoa` would be faster and is one more dependency; the row number is
    // written once per cell and the digits are few.
    out.push_str(&(row + 1).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(text: &str) -> String {
        let mut out = String::new();
        escape(text, &mut out);
        out
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(esc("Licencia anual"), "Licencia anual");
    }

    #[test]
    fn escapes_the_five_that_matter() {
        assert_eq!(
            esc(r#"a & b < c > d " e ' f"#),
            "a &amp; b &lt; c &gt; d &quot; e &apos; f"
        );
    }

    #[test]
    fn escapes_a_cdata_close_because_of_the_angle_bracket() {
        assert_eq!(esc("]]>"), "]]&gt;");
    }

    #[test]
    fn keeps_accents_and_anything_else_outside_ascii() {
        // UTF-8 throughout: a spreadsheet full of Spanish is the normal case,
        // not a character-set question.
        assert_eq!(esc("Días — 日本語 — ñ"), "Días — 日本語 — ñ");
    }

    #[test]
    fn keeps_the_three_control_characters_xml_allows() {
        assert_eq!(esc("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn writes_the_other_control_characters_the_way_excel_does() {
        // A NUL or a bell in a description comes from a database more often
        // than anyone would like. Left alone it makes the file unopenable.
        assert_eq!(esc("a\u{0}b\u{7}c"), "a_x0000_b_x0007_c");
    }

    fn column(index: u32) -> String {
        let mut out = String::new();
        column_name(index, &mut out);
        out
    }

    #[test]
    fn names_the_first_twenty_six_columns_with_one_letter() {
        assert_eq!(column(0), "A");
        assert_eq!(column(25), "Z");
    }

    #[test]
    fn carries_into_two_letters_without_a_zero_digit() {
        // The bijective part: after Z comes AA, not BA.
        assert_eq!(column(26), "AA");
        assert_eq!(column(27), "AB");
        assert_eq!(column(51), "AZ");
        assert_eq!(column(52), "BA");
    }

    #[test]
    fn names_the_last_column_excel_has() {
        // 16,384 columns, zero-based, is XFD. If this ever fails the buffer in
        // `column_name` is too small and the panic will be a slice index.
        assert_eq!(column(16_383), "XFD");
    }

    #[test]
    fn builds_a_cell_reference_from_row_and_column() {
        let mut out = String::new();
        cell_ref(6, 2, &mut out);
        assert_eq!(out, "C7");
    }
}
