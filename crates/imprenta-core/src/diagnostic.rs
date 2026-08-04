//! Build-time diagnostics.
//!
//! The engine knows when a cell overflowed, a row was orphaned or text was
//! clipped. Today those are silent visual defects found by whoever opens the
//! PDF; here they are reported against the template that caused them, so they
//! can surface in the dev server and fail CI.
//!
//! The load-bearing behaviour is **aggregation**: one clipped column in a
//! 9,000-page document is one diagnostic listing the affected pages, not nine
//! thousand identical lines.

use std::collections::BTreeMap;

/// Where in the author's source a diagnostic originates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
}

impl SourceLocation {
    pub fn new(file: impl Into<String>, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// One reported problem, possibly spanning many pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable kebab-case identifier, e.g. `text-clipped`. Machine-readable so
    /// a project can allow or deny specific classes of problem.
    pub code: String,
    pub message: String,
    pub location: Option<SourceLocation>,
    /// 1-indexed pages on which the problem occurred, ascending and unique.
    pub pages: Vec<u32>,
    /// What the author should try instead.
    pub hint: Option<String>,
}

impl Diagnostic {
    fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            location: None,
            pages: Vec::new(),
            hint: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn at(mut self, location: SourceLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub fn on_page(mut self, page: u32) -> Self {
        if let Err(i) = self.pages.binary_search(&page) {
            self.pages.insert(i, page);
        }
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}[{}]", self.severity, self.code)?;
        if let Some(loc) = &self.location {
            write!(f, " {}:{}", loc.file, loc.line)?;
        }
        write!(f, ": {}", self.message)?;
        if !self.pages.is_empty() {
            let list: Vec<String> = self.pages.iter().map(u32::to_string).collect();
            write!(f, " (pages {})", list.join(", "))?;
        }
        if let Some(hint) = &self.hint {
            write!(f, " — {hint}")?;
        }
        Ok(())
    }
}

/// Collects diagnostics, merging repeats of the same problem at the same site.
#[derive(Debug, Default)]
pub struct Diagnostics {
    /// Keyed by (code, location) so repeats merge. `BTreeMap` keeps the output
    /// order deterministic, which matters because these end up in CI logs and
    /// in snapshot tests.
    by_site: BTreeMap<(String, Option<SourceLocation>), Diagnostic>,
}

impl Diagnostics {
    /// Records a diagnostic, merging it into an existing one for the same
    /// code and source location.
    pub fn report(&mut self, diagnostic: Diagnostic) {
        let key = (diagnostic.code.clone(), diagnostic.location.clone());
        match self.by_site.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(diagnostic);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = slot.get_mut();
                for page in diagnostic.pages {
                    if let Err(i) = existing.pages.binary_search(&page) {
                        existing.pages.insert(i, page);
                    }
                }
                // A site that both warns and errors is an error, and the
                // error's message is the one worth showing.
                if diagnostic.severity > existing.severity {
                    existing.severity = diagnostic.severity;
                    existing.message = diagnostic.message;
                    existing.hint = diagnostic.hint;
                }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.by_site.values()
    }

    pub fn len(&self) -> usize {
        self.by_site.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_site.is_empty()
    }

    /// Whether the render should be treated as failed.
    ///
    /// `deny_warnings` is the CI switch: locally a warning is information,
    /// in CI it is a failure.
    pub fn should_fail(&self, deny_warnings: bool) -> bool {
        let threshold = if deny_warnings {
            Severity::Warning
        } else {
            Severity::Error
        };
        self.iter().any(|d| d.severity >= threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc() -> SourceLocation {
        SourceLocation::new("invoice.tsx", 47)
    }

    #[test]
    fn a_fresh_collector_reports_no_failure() {
        let diags = Diagnostics::default();
        assert!(diags.is_empty());
        assert!(!diags.should_fail(false));
        assert!(!diags.should_fail(true));
    }

    #[test]
    fn warnings_alone_do_not_fail_the_build() {
        let mut diags = Diagnostics::default();
        diags.report(Diagnostic::warning(
            "text-clipped",
            "'Description' was clipped",
        ));

        assert_eq!(diags.len(), 1);
        assert!(!diags.should_fail(false));
    }

    #[test]
    fn an_error_fails_the_build() {
        let mut diags = Diagnostics::default();
        diags.report(Diagnostic::error(
            "column-overflow",
            "'Total' overflows by 3mm",
        ));

        assert!(diags.should_fail(false));
    }

    #[test]
    fn deny_warnings_turns_a_warning_into_a_failure() {
        let mut diags = Diagnostics::default();
        diags.report(Diagnostic::warning(
            "text-clipped",
            "'Description' was clipped",
        ));

        assert!(diags.should_fail(true));
    }

    #[test]
    fn the_same_problem_at_the_same_site_merges_into_one_diagnostic() {
        // A clipped column in a 9,000-page ledger must not emit 9,000 lines.
        let mut diags = Diagnostics::default();
        for page in [3, 7, 12] {
            diags.report(
                Diagnostic::warning("text-clipped", "'Description' was clipped")
                    .at(loc())
                    .on_page(page),
            );
        }

        assert_eq!(diags.len(), 1);
        assert_eq!(diags.iter().next().unwrap().pages, vec![3, 7, 12]);
    }

    #[test]
    fn merged_pages_are_sorted_and_deduplicated() {
        let mut diags = Diagnostics::default();
        for page in [12, 3, 7, 3] {
            diags.report(
                Diagnostic::warning("text-clipped", "clipped")
                    .at(loc())
                    .on_page(page),
            );
        }

        assert_eq!(diags.iter().next().unwrap().pages, vec![3, 7, 12]);
    }

    #[test]
    fn the_same_code_at_a_different_site_stays_separate() {
        let mut diags = Diagnostics::default();
        diags.report(
            Diagnostic::warning("text-clipped", "clipped")
                .at(loc())
                .on_page(1),
        );
        diags.report(
            Diagnostic::warning("text-clipped", "clipped")
                .at(SourceLocation::new("ledger.tsx", 88))
                .on_page(1),
        );

        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn an_error_wins_over_a_warning_when_merging() {
        // If the same site both warns and errors, the site is an error.
        let mut diags = Diagnostics::default();
        diags.report(Diagnostic::warning("column-overflow", "tight").at(loc()));
        diags.report(Diagnostic::error("column-overflow", "overflows").at(loc()));

        assert_eq!(diags.len(), 1);
        assert_eq!(diags.iter().next().unwrap().severity, Severity::Error);
        assert!(diags.should_fail(false));
    }

    #[test]
    fn formats_with_location_pages_and_hint() {
        let d = Diagnostic::warning("text-clipped", "'Description' was clipped")
            .at(loc())
            .on_page(3)
            .on_page(7)
            .with_hint(r#"try overflow="wrap""#);

        let s = d.to_string();
        assert!(s.contains("warning"), "{s}");
        assert!(s.contains("text-clipped"), "{s}");
        assert!(s.contains("invoice.tsx:47"), "{s}");
        assert!(s.contains("'Description' was clipped"), "{s}");
        assert!(s.contains("pages 3, 7"), "{s}");
        assert!(s.contains(r#"try overflow="wrap""#), "{s}");
    }

    #[test]
    fn formats_without_a_location_or_pages() {
        let s = Diagnostic::error("no-fonts", "no fonts registered").to_string();
        assert!(s.contains("error"), "{s}");
        assert!(s.contains("no fonts registered"), "{s}");
        assert!(!s.contains("pages"), "{s}");
    }
}
