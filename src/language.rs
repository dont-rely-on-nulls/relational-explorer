/// Envelope-aware input classification and rewriting for the Sakura
/// S-expression protocol.  Commands sent to the server are `(tag payload...)`
/// where `tag` is one of the five sublanguages.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Drl,
    Ddl,
    Dml,
    Icl,
    Dcl,
    Scl,
}

impl Tag {
    #[allow(dead_code)]
    pub const ALL: &[Tag] = &[Tag::Drl, Tag::Ddl, Tag::Dml, Tag::Icl, Tag::Dcl, Tag::Scl];

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Tag::Drl => "drl",
            Tag::Ddl => "ddl",
            Tag::Dml => "dml",
            Tag::Icl => "icl",
            Tag::Dcl => "dcl",
            Tag::Scl => "scl",
        }
    }

    pub fn from_str(s: &str) -> Option<Tag> {
        match s {
            "drl" => Some(Tag::Drl),
            "ddl" => Some(Tag::Ddl),
            "dml" => Some(Tag::Dml),
            "icl" => Some(Tag::Icl),
            "dcl" => Some(Tag::Dcl),
            "scl" => Some(Tag::Scl),
            _ => None,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum InputClassification {
    /// Well-formed envelope with a known sublanguage tag.
    Envelope(Tag),
    /// Looks like `(tag ...)` but the tag is not a known sublanguage.
    UnknownTag(String),
    /// Not in envelope form — the server's legacy cascade parser will handle it.
    Legacy,
    /// A recognised client-side shortcut; the `String` is the replacement to send.
    Shortcut(String),
    /// The input is not valid S-expression syntax.
    MalformedSexp(String),
}

/// Classify raw user input without coupling to each sublanguage's AST.
pub fn classify(input: &str) -> InputClassification {
    let trimmed = input.trim();

    // Client-side shortcuts
    if trimmed == "(schema)" {
        return InputClassification::Shortcut("(drl (Base sakura:attribute))".to_string());
    }

    // Try to parse as an S-expression
    let sexp = match sexp::parse(trimmed) {
        Ok(s) => s,
        Err(e) => return InputClassification::MalformedSexp(e.to_string()),
    };

    // Check for envelope shape: (tag payload...)
    if let sexp::Sexp::List(items) = &sexp {
        if let Some(sexp::Sexp::Atom(sexp::Atom::S(tag))) = items.first() {
            return match Tag::from_str(tag.as_str()) {
                Some(t) => InputClassification::Envelope(t),
                None => InputClassification::UnknownTag(tag.clone()),
            };
        }
    }

    InputClassification::Legacy
}

/// Rewrite client shortcuts, passing everything else through unchanged.
pub fn rewrite(input: &str) -> String {
    let trimmed = input.trim();
    // Fast path: only known shortcut is (schema)
    if trimmed == "(schema)" {
        "(drl (Base sakura:attribute))".to_string()
    } else {
        input.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_shortcut() {
        let out = rewrite("(schema)");
        assert_eq!(out, "(drl (Base sakura:attribute))");
    }

    #[test]
    fn known_envelope() {
        assert!(matches!(
            classify("(ddl (CreateDatabase \"test\"))"),
            InputClassification::Envelope(Tag::Ddl)
        ));
        assert!(matches!(
            classify("(drl (Base sakura:attribute))"),
            InputClassification::Envelope(Tag::Drl)
        ));
        assert!(matches!(
            classify("(dcl GetHead)"),
            InputClassification::Envelope(Tag::Dcl)
        ));
        assert!(matches!(
            classify("(scl (Begin (query (Base sakura:attribute)) (limit 3)))"),
            InputClassification::Envelope(Tag::Scl)
        ));
    }

    #[test]
    fn unknown_tag() {
        assert!(
            matches!(classify("(foo bar)"), InputClassification::UnknownTag(ref s) if s == "foo")
        );
    }

    #[test]
    fn malformed_sexp() {
        assert!(matches!(
            classify("(broken"),
            InputClassification::MalformedSexp(_)
        ));
    }

    #[test]
    fn passthrough() {
        let raw = "(ddl (CreateDatabase \"test\"))";
        assert_eq!(rewrite(raw), raw);
    }
}
