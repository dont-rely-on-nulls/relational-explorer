use ascii_table::AsciiTable;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Metadata common to every server response.
#[derive(Debug, Clone)]
pub struct ResponseMeta {
    pub db_hash: String,
    pub db_name: String,
    pub branch: String,
}

#[derive(Debug)]
pub enum ServerResponse {
    Relation {
        name: String,
        schema: Vec<SchemaField>,
        rows: Vec<Vec<(String, String)>>,
        row_count: u32,
        truncated: bool,
        meta: ResponseMeta,
    },
    Ok {
        message: String,
        meta: ResponseMeta,
    },
    Error {
        kind: String,
        message: String,
        meta: ResponseMeta,
    },
    Cursor {
        cursor_id: String,
        rows: Vec<Vec<(String, String)>>,
        row_count: u32,
        has_more: bool,
        meta: ResponseMeta,
    },
}

#[derive(Debug)]
pub struct SchemaField {
    pub attr: String,
    #[allow(dead_code)]
    pub domain: String,
}

impl ServerResponse {
    pub fn meta(&self) -> &ResponseMeta {
        match self {
            ServerResponse::Relation { meta, .. }
            | ServerResponse::Ok { meta, .. }
            | ServerResponse::Error { meta, .. }
            | ServerResponse::Cursor { meta, .. } => meta,
        }
    }
}

pub fn format_response(resp: &ServerResponse) -> String {
    match resp {
        ServerResponse::Relation {
            name,
            schema,
            rows,
            row_count,
            truncated,
            ..
        } => {
            let table = render_table(schema, rows);
            let suffix = if *truncated {
                format!("({} rows, truncated)", row_count)
            } else {
                format!("({} rows)", row_count)
            };
            format!("{}\n{}{}", name, table, suffix)
        }
        ServerResponse::Ok { message, .. } => format!("OK  {}", message),
        ServerResponse::Error { kind, message, .. } => format!("{}: {}", kind, message),
        ServerResponse::Cursor {
            cursor_id,
            rows,
            row_count,
            has_more,
            ..
        } => {
            let schema: Vec<SchemaField> = rows
                .first()
                .map(|row| {
                    row.iter()
                        .map(|(k, _)| SchemaField {
                            attr: k.clone(),
                            domain: String::new(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let table = render_table(&schema, rows);
            let short_id = if cursor_id.len() > 8 {
                &cursor_id[..8]
            } else {
                cursor_id
            };
            format!(
                "{}({} rows, cursor: {}, has_more: {})",
                table, row_count, short_id, has_more
            )
        }
    }
}

pub fn error_parts(resp: &ServerResponse) -> Option<(&str, &str)> {
    match resp {
        ServerResponse::Error { kind, message, .. } => Some((kind, message)),
        _ => None,
    }
}

fn render_table(schema: &[SchemaField], rows: &[Vec<(String, String)>]) -> String {
    if schema.is_empty() {
        return String::new();
    }

    let mut table = AsciiTable::default();
    for (i, field) in schema.iter().enumerate() {
        table.column(i).set_header(&*field.attr);
    }

    let data: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| {
            schema
                .iter()
                .map(|field| {
                    row.iter()
                        .find(|(k, _)| k == &field.attr)
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("?")
                })
                .collect()
        })
        .collect();

    let mut output = Vec::new();
    table.writeln(&mut output, &data).ok();
    String::from_utf8(output).unwrap_or_default()
}

// --- S-expression parsing ---

fn atom_string(s: &sexp::Sexp) -> Option<String> {
    match s {
        sexp::Sexp::Atom(sexp::Atom::S(s)) => Some(s.clone()),
        sexp::Sexp::Atom(sexp::Atom::I(n)) => Some(n.to_string()),
        sexp::Sexp::Atom(sexp::Atom::F(f)) => Some(f.to_string()),
        sexp::Sexp::List(_) => None,
    }
}

fn atom_string_debug(s: &sexp::Sexp) -> String {
    match s {
        sexp::Sexp::Atom(sexp::Atom::S(s)) => s.clone(),
        sexp::Sexp::Atom(sexp::Atom::I(n)) => n.to_string(),
        sexp::Sexp::Atom(sexp::Atom::F(f)) => f.to_string(),
        sexp::Sexp::List(l) => format!("(list with {} items)", l.len()),
    }
}

/// Find the value for `key` in a plist-style list: `((key val) ...)`.
fn get_field<'a>(items: &'a [sexp::Sexp], key: &str) -> Option<&'a sexp::Sexp> {
    items.iter().find_map(|item| {
        if let sexp::Sexp::List(pair) = item {
            if pair.len() == 2 && atom_string(&pair[0]).as_deref() == Some(key) {
                return Some(&pair[1]);
            }
        }
        None
    })
}

fn get_str(items: &[sexp::Sexp], key: &str) -> Option<String> {
    get_field(items, key).and_then(|v| atom_string(v))
}

/// Extract the common db_hash / db_name / branch metadata from a response's fields.
fn parse_meta(rest: &[sexp::Sexp]) -> ResponseMeta {
    ResponseMeta {
        db_hash: get_str(rest, "db_hash").unwrap_or_default(),
        db_name: get_str(rest, "db_name").unwrap_or_default(),
        branch: get_str(rest, "branch").unwrap_or_else(|| "--".to_string()),
    }
}

/// Parse a list of `((key value) ...)` pairs from the sexp rows field.
fn parse_rows(rest: &[sexp::Sexp]) -> Vec<Vec<(String, String)>> {
    match get_field(rest, "rows") {
        Some(sexp::Sexp::List(row_list)) => row_list
            .iter()
            .filter_map(|row| {
                if let sexp::Sexp::List(pairs) = row {
                    Some(parse_kv_pairs(pairs))
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

/// Parse key-value pairs from a list of `(key value)` S-expressions.
fn parse_kv_pairs(pairs: &[sexp::Sexp]) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter_map(|pair| {
            if let sexp::Sexp::List(p) = pair {
                if p.len() == 2 {
                    let k = atom_string(&p[0])?;
                    let v = atom_string(&p[1])?;
                    return Some((k, v));
                }
            }
            None
        })
        .collect()
}

/// Insert a space before any `"` that is not preceded by a sexp delimiter.
/// OCaml's Sexplib serialises atoms and quoted strings without an intervening
/// space, but the `sexp` crate's parser doesn't treat `"` as a token boundary.
fn normalize_sexp_spacing(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut prev = None::<char>;
    for ch in s.chars() {
        if ch == '"' {
            if let Some(p) = prev {
                if p != '(' && p != ')' && p != '\\' && !p.is_whitespace() {
                    out.push(' ');
                }
            }
        }
        prev = Some(ch);
        out.push(ch);
    }
    out
}

fn parse_response(s: &str) -> std::io::Result<ServerResponse> {
    let normalized = normalize_sexp_spacing(s);
    let sexp = sexp::parse(&normalized)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let items = match &sexp {
        sexp::Sexp::List(items) => items,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected list",
            ))
        }
    };

    let tag = items
        .first()
        .and_then(|t| atom_string(t))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing tag"))?;

    let rest = &items[1..];
    let meta = parse_meta(rest);

    match tag.as_str() {
        "ok" => {
            let message = match get_field(rest, "message") {
                Some(field) => atom_string_debug(field),
                None => "(message field missing)".to_string(),
            };
            Ok(ServerResponse::Ok { message, meta })
        }
        "error" => {
            let raw = match get_field(rest, "message") {
                Some(field) => atom_string_debug(field),
                None => format!(
                    "(message field malformed or missing; full response: {:?})",
                    rest
                ),
            };
            let (kind, message) = match raw.split_once(": ") {
                Some((k, m)) => (k.to_string(), m.to_string()),
                None => (String::from("Error"), raw),
            };
            Ok(ServerResponse::Error {
                kind,
                message,
                meta,
            })
        }
        "relation" => {
            let name = get_str(rest, "name").unwrap_or_default();
            let row_count = get_str(rest, "row_count")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let truncated = get_str(rest, "truncated").as_deref() == Some("true");

            let schema = match get_field(rest, "schema") {
                Some(sexp::Sexp::List(pairs)) => pairs
                    .iter()
                    .filter_map(|pair| {
                        if let sexp::Sexp::List(p) = pair {
                            if p.len() == 2 {
                                let attr = atom_string(&p[0])?;
                                let domain = atom_string(&p[1])?;
                                return Some(SchemaField { attr, domain });
                            }
                        }
                        None
                    })
                    .collect(),
                _ => vec![],
            };

            let rows = parse_rows(rest);

            Ok(ServerResponse::Relation {
                name,
                schema,
                rows,
                row_count,
                truncated,
                meta,
            })
        }
        "cursor" => {
            let cursor_id = get_str(rest, "id").unwrap_or_default();
            let row_count = get_str(rest, "row_count")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let has_more = get_str(rest, "has_more").as_deref() == Some("true");
            let rows = parse_rows(rest);

            Ok(ServerResponse::Cursor {
                cursor_id,
                rows,
                row_count,
                has_more,
                meta,
            })
        }
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unknown response tag: {}", other),
        )),
    }
}

pub struct Connection {
    writer: Box<dyn Write + Send>,
    reader: BufReader<Box<dyn Read + Send>>,
}

/// Determines whether `addr` looks like a Unix socket path (contains `/` or
/// ends with `.sock`) versus a TCP `host:port` address.
pub fn is_unix_socket(addr: &str) -> bool {
    addr.contains('/') || addr.ends_with(".sock")
}

impl Connection {
    /// Connect to the server at `addr`.
    ///
    /// If `addr` looks like a filesystem path (contains `/` or ends with
    /// `.sock`), a Unix domain socket is used.  Otherwise it is treated as a
    /// TCP `host:port` address.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        if is_unix_socket(addr) {
            let stream = UnixStream::connect(Path::new(addr))?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
            let reader_stream = stream.try_clone()?;
            Self::from_streams(stream, reader_stream)
        } else {
            let stream = TcpStream::connect(addr)?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
            let reader_stream = stream.try_clone()?;
            Self::from_streams(stream, reader_stream)
        }
    }

    fn from_streams<W: Write + Send + 'static, R: Read + Send + 'static>(
        writer: W,
        reader: R,
    ) -> std::io::Result<Self> {
        Ok(Self {
            writer: Box::new(writer),
            reader: BufReader::new(Box::new(reader)),
        })
    }

    pub fn send(&mut self, cmd: &str) -> std::io::Result<ServerResponse> {
        writeln!(self.writer, "{}", cmd)?;
        self.writer.flush()?;

        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        parse_response(line.trim())
    }
}
