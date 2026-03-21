use ascii_table::AsciiTable;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

#[derive(Debug)]
pub enum ServerResponse {
    Relation {
        name: String,
        schema: Vec<SchemaField>,
        rows: Vec<Vec<(String, String)>>,
        row_count: u32,
        truncated: bool,
        db_hash: String,
        db_name: String,
        branch: String,
    },
    Ok {
        message: String,
        db_hash: String,
        db_name: String,
        branch: String,
    },
    Error {
        kind: String,
        message: String,
        db_hash: String,
        db_name: String,
        branch: String,
    },
}

#[derive(Debug)]
pub struct SchemaField {
    pub attr: String,
    pub domain: String,
}

impl ServerResponse {
    pub fn db_hash(&self) -> &str {
        match self {
            ServerResponse::Relation { db_hash, .. } => db_hash,
            ServerResponse::Ok { db_hash, .. } => db_hash,
            ServerResponse::Error { db_hash, .. } => db_hash,
        }
    }

    pub fn db_name(&self) -> &str {
        match self {
            ServerResponse::Relation { db_name, .. } => db_name,
            ServerResponse::Ok { db_name, .. } => db_name,
            ServerResponse::Error { db_name, .. } => db_name,
        }
    }

    pub fn branch(&self) -> &str {
        match self {
            ServerResponse::Relation { branch, .. } => branch,
            ServerResponse::Ok { branch, .. } => branch,
            ServerResponse::Error { branch, .. } => branch,
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
    }
}

pub fn error_parts(resp: &ServerResponse) -> Option<(&str, &str)> {
    match resp {
        ServerResponse::Error { kind, message, .. } => Some((kind.as_str(), message.as_str())),
        _ => None,
    }
}

fn render_table(schema: &[SchemaField], rows: &[Vec<(String, String)>]) -> String {
    if schema.is_empty() {
        return String::new();
    }

    let mut table = AsciiTable::default();
    for (i, field) in schema.iter().enumerate() {
        table.column(i).set_header(field.attr.as_str());
    }

    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            schema
                .iter()
                .map(|field| {
                    row.iter()
                        .find(|(k, _)| k == &field.attr)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| String::from("?"))
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

fn parse_response(s: &str) -> std::io::Result<ServerResponse> {
    let sexp = sexp::parse(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let items = match &sexp {
        sexp::Sexp::List(items) => items,
        _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "expected list")),
    };

    let tag = items
        .first()
        .and_then(|t| atom_string(t))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing tag"))?;

    let rest = &items[1..];

    match tag.as_str() {
        "ok" => {
            let message = match get_field(rest, "message") {
                Some(field) => atom_string_debug(field),
                None => "(message field missing)".to_string(),
            };
            let db_hash = get_str(rest, "db_hash").unwrap_or_default();
            let db_name = get_str(rest, "db_name").unwrap_or_default();
            let branch  = get_str(rest, "branch").unwrap_or_else(|| "--".to_string());
            Ok(ServerResponse::Ok { message, db_hash, db_name, branch })
        }
        "error" => {
            let raw = match get_field(rest, "message") {
                Some(field) => atom_string_debug(field),
                None => format!("(message field malformed or missing; full response: {:?})", rest),
            };
            let (kind, message) = match raw.split_once(": ") {
                Some((k, m)) => (k.to_string(), m.to_string()),
                None => (String::from("Error"), raw),
            };
            let db_hash = get_str(rest, "db_hash").unwrap_or_default();
            let db_name = get_str(rest, "db_name").unwrap_or_default();
            let branch  = get_str(rest, "branch").unwrap_or_else(|| "--".to_string());
            Ok(ServerResponse::Error { kind, message, db_hash, db_name, branch })
        }
        "relation" => {
            let name    = get_str(rest, "name").unwrap_or_default();
            let db_hash = get_str(rest, "db_hash").unwrap_or_default();
            let db_name = get_str(rest, "db_name").unwrap_or_default();
            let branch  = get_str(rest, "branch").unwrap_or_else(|| "--".to_string());
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

            let rows = match get_field(rest, "rows") {
                Some(sexp::Sexp::List(row_list)) => row_list
                    .iter()
                    .filter_map(|row| {
                        if let sexp::Sexp::List(pairs) = row {
                            Some(
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
                                    .collect(),
                            )
                        } else {
                            None
                        }
                    })
                    .collect(),
                _ => vec![],
            };

            Ok(ServerResponse::Relation {
                name,
                schema,
                rows,
                row_count,
                truncated,
                db_hash,
                db_name,
                branch,
            })
        }
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unknown response tag: {}", other),
        )),

    }
}

pub struct Connection {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

impl Connection {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }

    pub fn send(&mut self, cmd: &str) -> std::io::Result<ServerResponse> {
        writeln!(self.stream, "{}", cmd)?;
        self.stream.flush()?;

        // Set read timeout to prevent indefinite blocking
        self.stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))?;

        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        parse_response(line.trim())
    }
}
