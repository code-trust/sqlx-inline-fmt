use std::collections::{BTreeMap, btree_map::Entry};
use std::fs;
use std::io::Write as _;
use std::ops::Range;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use litrs::StringLit;
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::Mode;

type ByteSpan = Range<usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    Unchanged,
    WouldChange,
    Changed,
}

impl FileChange {
    pub fn did_change(self) -> bool {
        matches!(self, Self::WouldChange | Self::Changed)
    }
}

#[derive(Debug, Clone)]
struct LiteralCapture {
    span: ByteSpan,
    literal: StringLit<String>,
    line: usize,
    indent: String,
}

#[derive(Debug, Clone)]
struct Replacement {
    span: ByteSpan,
    text: String,
}

#[derive(Debug)]
enum FormatterOutcome {
    Formatted(String),
    FormattedWithWarnings { formatted: String, stderr: String },
    FormatterErrored { stderr: String },
}

#[derive(Debug)]
struct FormatterCommandOutput {
    stdout: String,
    stderr: String,
    success: bool,
}

pub struct Formatter {
    parser: Parser,
    query: Query,
    command: Vec<String>,
    had_errors: bool,
    ignore_exit_code: bool,
}

impl Formatter {
    pub fn new(command: Vec<String>, ignore_exit_code: bool) -> Result<Self> {
        let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .context("setting tree-sitter rust language")?;

        let query = Query::new(&language, include_str!("sqlx-macros.scm"))
            .context("compiling tree-sitter query")?;

        Ok(Self {
            parser,
            query,
            command,
            had_errors: false,
            ignore_exit_code,
        })
    }

    pub fn had_errors(&self) -> bool {
        self.had_errors
    }

    pub fn format_path(&mut self, path: &Path, mode: Mode) -> Result<FileChange> {
        let source = fs::read_to_string(path).context("reading source file")?;
        let replacements = replacements(
            path,
            &source,
            &mut self.parser,
            &self.query,
            &self.command,
            self.ignore_exit_code,
            &mut self.had_errors,
        )?;

        if replacements.is_empty() {
            return Ok(FileChange::Unchanged);
        }

        if mode.is_check() {
            return Ok(FileChange::WouldChange);
        }

        let updated = apply_replacements(replacements, &source);
        fs::write(path, updated).context("writing formatted source")?;

        Ok(FileChange::Changed)
    }
}

fn replacements(
    path: &Path,
    src: &str,
    parser: &mut Parser,
    query: &Query,
    command: &[String],
    ignore_exit_code: bool,
    had_errors: &mut bool,
) -> Result<Vec<Replacement>> {
    let captures = collect_literal_captures(path, src, parser, query, had_errors)?;

    let mut replacements = Vec::with_capacity(captures.len());

    for cap in captures {
        match format_literal_with_command(&cap, command, ignore_exit_code)? {
            FormatterOutcome::Formatted(formatted) if formatted != cap.literal.raw_input() => {
                replacements.push(Replacement {
                    span: cap.span.clone(),
                    text: formatted,
                });
            }
            FormatterOutcome::Formatted(_) => {}
            FormatterOutcome::FormattedWithWarnings { formatted, stderr } => {
                eprintln!(
                    "[WARN] {}:{}: formatter exited with non-zero status (ignored)",
                    path.display(),
                    cap.line
                );

                if !stderr.trim().is_empty() {
                    print_indented_lines(stderr.trim_end());
                }

                if formatted != cap.literal.raw_input() {
                    replacements.push(Replacement {
                        span: cap.span.clone(),
                        text: formatted,
                    });
                }
            }
            FormatterOutcome::FormatterErrored { stderr } => {
                eprintln!(
                    "[WARN] {}:{}: error from formatter",
                    path.display(),
                    cap.line
                );

                if !stderr.trim().is_empty() {
                    print_indented_lines(stderr.trim_end());
                }
                if !ignore_exit_code {
                    *had_errors = true;
                }
            }
        }
    }

    Ok(replacements)
}

fn apply_replacements(mut replacements: Vec<Replacement>, src: &str) -> String {
    replacements.sort_by_key(|r| r.span.start);

    let mut out = String::with_capacity(src.len() + 1024);
    let mut cursor = 0;

    for Replacement { span, text } in replacements {
        out.push_str(&src[cursor..span.start]);
        out.push_str(&text);
        cursor = span.end;
    }

    out.push_str(&src[cursor..]);
    out
}

fn format_literal_with_command(
    literal: &LiteralCapture,
    command: &[String],
    ignore_exit_code: bool,
) -> Result<FormatterOutcome> {
    let content = normalized_content_for_formatter(&literal.literal);

    let output = run_formatter(&content, command)?;

    if output.success {
        return Ok(FormatterOutcome::Formatted(rewrite_literal(
            &output.stdout,
            literal,
        )));
    }

    if ignore_exit_code {
        return Ok(FormatterOutcome::FormattedWithWarnings {
            formatted: rewrite_literal(&output.stdout, literal),
            stderr: output.stderr,
        });
    }

    Ok(FormatterOutcome::FormatterErrored {
        stderr: output.stderr,
    })
}

fn rewrite_literal(formatted: &str, literal: &LiteralCapture) -> String {
    let trimmed_end = formatted.trim_end_matches('\n');

    if !trimmed_end.contains('\n') {
        let mut out = String::from('"');
        out.push_str(&escape_regular_string(trimmed_end));
        out.push('"');
        return out;
    }

    let hashes = "#".repeat(required_raw_hashes(formatted));
    let mut out =
        String::with_capacity(formatted.len() + literal.indent.len() * 2 + hashes.len() * 2 + 4);

    out.push('r');
    out.push_str(&hashes);
    out.push('"');
    out.push('\n');

    let body = indent_block(formatted.trim_matches('\n'), &literal.indent);
    if !body.is_empty() {
        out.push_str(&body);
    }

    if formatted.ends_with('\n') {
        out.push('\n');
    }

    out.push_str(&literal.indent);
    out.push('"');
    out.push_str(&hashes);
    out
}

fn escape_regular_string(formatted: &str) -> String {
    let mut out = String::with_capacity(formatted.len());

    for ch in formatted.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }

    out
}

fn normalized_content_for_formatter(lit: &StringLit<String>) -> String {
    let raw = lit.raw_input();
    let first_quote = raw
        .find('"')
        .expect("string literal must contain opening quote");
    let last_quote = raw
        .rfind('"')
        .expect("string literal must contain closing quote");
    let inner_raw = &raw[first_quote + 1..last_quote];

    let leading_newline = inner_raw.starts_with('\n');
    let closing_indent = inner_raw
        .rsplit_once('\n')
        .and_then(|(_, last)| {
            last.chars()
                .all(|c| matches!(c, ' ' | '\t'))
                .then_some(last)
        })
        .unwrap_or_default();

    let mut value = lit.value().to_owned();

    if leading_newline && value.starts_with('\n') {
        value.remove(0);
    }

    if !closing_indent.is_empty() && value.ends_with(closing_indent) {
        value.truncate(value.len() - closing_indent.len());
    }

    if leading_newline
        && let Some(line_indent) = value.lines().find_map(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            (!trimmed.is_empty()).then(|| line[..line.len() - trimmed.len()].to_string())
        })
        && !line_indent.is_empty()
    {
        return strip_indent(&value, &line_indent);
    }

    value
}

fn collect_literal_captures(
    path: &Path,
    src: &str,
    parser: &mut Parser,
    query: &Query,
    had_errors: &mut bool,
) -> Result<Vec<LiteralCapture>> {
    let tree = parser
        .parse(src, None)
        .context("parsing source with tree-sitter")?;

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), src.as_bytes());
    let capture_names = query.capture_names();
    let mut literals: BTreeMap<(usize, usize), Node> = BTreeMap::new();

    while let Some(m) = matches.next() {
        let mut invocation: Option<(usize, usize)> = None;
        let mut literal_node: Option<Node> = None;

        for capture in m.captures.iter().copied() {
            let name = &capture_names[capture.index as usize];

            match &**name {
                "inv" => {
                    invocation = Some((capture.node.start_byte(), capture.node.end_byte()));
                }
                "lit" | "raw" => {
                    let candidate = capture.node;
                    literal_node = Some(literal_node.map_or(candidate, |current| {
                        if node_span(candidate) < node_span(current) {
                            candidate
                        } else {
                            current
                        }
                    }));
                }
                _ => {}
            }
        }

        let (Some(invocation), Some(node)) = (invocation, literal_node) else {
            continue;
        };

        if !is_string_literal(node.kind()) {
            continue;
        }

        match literals.entry(invocation) {
            Entry::Vacant(entry) => {
                entry.insert(node);
            }
            Entry::Occupied(mut entry) => {
                let current = entry.get();
                if node_span(node) < node_span(*current) {
                    entry.insert(node);
                }
            }
        }
    }

    let mut captures = Vec::with_capacity(literals.len());

    for node in literals.into_values() {
        if let Some(capture) = literal_capture_from_node(path, src, node, had_errors) {
            captures.push(capture);
        }
    }

    Ok(captures)
}

fn is_string_literal(kind: &str) -> bool {
    matches!(kind, "raw_string_literal" | "string_literal")
}

fn node_span(node: Node) -> (usize, usize) {
    (node.start_byte(), node.end_byte())
}

fn literal_capture_from_node(
    path: &Path,
    src: &str,
    node: Node,
    had_errors: &mut bool,
) -> Option<LiteralCapture> {
    let span = node.byte_range();
    let text = &src[span.clone()];

    match StringLit::parse(text) {
        Ok(literal) => Some(LiteralCapture {
            span: span.clone(),
            literal: literal.into_owned(),
            line: node.start_position().row + 1,
            indent: literal_indent(src, span.start),
        }),
        Err(parse_err) => {
            eprintln!("[ERROR] {}: failed to parse string literal", path.display());
            print_indented_lines(text.trim_end_matches('\n'));
            eprintln!("{parse_err}");
            *had_errors = true;
            None
        }
    }
}

fn run_formatter(input: &str, command: &[String]) -> Result<FormatterCommandOutput> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("formatter command cannot be empty"))?;

    let mut child = Command::new(program)
        .args(args)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning formatter command")?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(input.as_bytes())
            .context("writing formatter input")?;

        if !input.ends_with('\n') {
            stdin
                .write_all(b"\n")
                .context("appending trailing newline to formatter input")?;
        }
    } else {
        return Err(anyhow!("failed to open stdin"));
    }

    let output = child
        .wait_with_output()
        .context("waiting for formatter command")?;

    let stdout = String::from_utf8(output.stdout).context("formatter command stdout not UTF-8")?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(FormatterCommandOutput {
        stdout,
        stderr,
        success: output.status.success(),
    })
}

fn strip_indent(block: &str, indent: &str) -> String {
    if block.is_empty() || indent.is_empty() {
        return block.to_owned();
    }

    let mut result = String::with_capacity(block.len());

    for chunk in block.split_inclusive('\n') {
        if let Some(line) = chunk.strip_suffix('\n') {
            if let Some(stripped) = line.strip_prefix(indent) {
                result.push_str(stripped);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        } else if let Some(stripped) = chunk.strip_prefix(indent) {
            result.push_str(stripped);
        } else {
            result.push_str(chunk);
        }
    }

    result
}

fn indent_block(block: &str, indent: &str) -> String {
    if block.is_empty() || indent.is_empty() {
        return block.to_owned();
    }

    let line_count = block.lines().count();
    let mut result = String::with_capacity(block.len() + indent.len() * line_count);

    for chunk in block.split_inclusive('\n') {
        if let Some(line) = chunk.strip_suffix('\n') {
            if !line.is_empty() {
                result.push_str(indent);
                result.push_str(line);
            }
            result.push('\n');
        } else if !chunk.is_empty() {
            result.push_str(indent);
            result.push_str(chunk);
        }
    }

    result
}

fn literal_indent(src: &str, literal_start: usize) -> String {
    let line_start = src[..literal_start].rfind('\n').map_or(0, |idx| idx + 1);
    src[line_start..literal_start]
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect()
}

fn required_raw_hashes(content: &str) -> usize {
    let mut hashes = 0;

    loop {
        let pattern = format!("\"{}", "#".repeat(hashes));

        if !content.contains(&pattern) {
            return hashes;
        }

        hashes += 1;
    }
}

fn print_indented_lines(s: &str) {
    for line in s.lines() {
        eprintln!("  {line}");
    }
}
