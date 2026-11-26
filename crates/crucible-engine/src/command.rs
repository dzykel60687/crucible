//! The command layer (D-022): tables of named commands with typed arguments.
//!
//! Every user action is a row in a table: name, arguments, one line of help, what kind of effect
//! it has, and the function that does it. The engine has a table for project edits; the app adds
//! its own (devices, diagnostics). The GUI, the in-app console and the `crucible` CLI all go
//! through [`parse`] and the same rows, so there is exactly one way to do each thing.
//!
//! Text is only the outside form. [`parse`] turns tokens into typed [`Value`]s once, at the edge,
//! and [`canonical`] turns them back into the one exact text form used by history and the journal.
//!
//! Text syntax, kept deliberately small:
//! - `name positional... --arg value --flag`. Positionals fill arguments in table order.
//! - Quotes: `"two words"` or `'two words'`. Inside double quotes, `""` is a literal quote.
//!   Backslashes are literal (Windows paths).
//! - `;` separates statements on one line. A line starting with `#` is a comment.

use crucible_diagnostics::{Code, DawError};
use serde_json::Value as Json;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Text,
    Number,
    Int,
    Bool,
    /// A track by name, position (`2`) or id (`#7`). Resolved to `#id` before the command runs.
    Track,
    Choice(&'static [&'static str]),
    /// Free text: every further word on the line joins it, so quotes are optional
    /// (`mark heard a click`). At most one per command.
    Words,
}

pub struct Arg {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub help: &'static str,
}

pub const fn req(name: &'static str, kind: Kind, help: &'static str) -> Arg {
    Arg { name, kind, required: true, help }
}

pub const fn opt(name: &'static str, kind: Kind, help: &'static str) -> Arg {
    Arg { name, kind, required: false, help }
}

/// What running a command does, which decides undo and journal handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads only.
    Read,
    /// Changes the project: undoable and journaled.
    Edit,
    /// Undo and redo: journaled, not itself undoable.
    History,
    /// Fills the engine's clipboard: always journaled so a later paste replays, never undoable.
    Clipboard,
    /// New, open, save: changes which project is loaded or where it lives.
    File,
    /// Outside the project: devices, plugins, diagnostics. Logged, never journaled.
    System,
}

pub struct Spec<C> {
    pub name: &'static str,
    pub args: &'static [Arg],
    pub help: &'static str,
    pub effect: Effect,
    pub run: fn(&mut C, &Args) -> Result<Reply, DawError>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Int(i64),
    Bool(bool),
}

/// Parsed arguments, by name. Only arguments that were given are present.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Args {
    values: Vec<(&'static str, Value)>,
}

impl Args {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
    pub fn set(&mut self, name: &'static str, value: Value) {
        match self.values.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.values.push((name, value)),
        }
    }
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Value::Text(s)) => Some(s),
            _ => None,
        }
    }
    pub fn number(&self, name: &str) -> Option<f64> {
        match self.get(name) {
            Some(Value::Number(n)) => Some(*n),
            Some(Value::Int(i)) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name) {
            Some(Value::Int(i)) => Some(*i),
            _ => None,
        }
    }
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.get(name) {
            Some(Value::Bool(b)) => Some(*b),
            _ => None,
        }
    }
    pub fn need_text(&self, name: &str) -> Result<&str, DawError> {
        self.text(name).ok_or_else(|| missing(name))
    }
    pub fn need_number(&self, name: &str) -> Result<f64, DawError> {
        self.number(name).ok_or_else(|| missing(name))
    }
    pub fn need_int(&self, name: &str) -> Result<i64, DawError> {
        self.int(name).ok_or_else(|| missing(name))
    }
}

fn missing(name: &str) -> DawError {
    DawError::new(Code::BadArgument, format!("missing argument '{name}'"))
}

fn bad(msg: String) -> DawError {
    DawError::new(Code::BadArgument, msg)
}

/// What a command returns: a line or two for people, and structured data for scripts and the GUI.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub text: String,
    pub data: Json,
    /// The command wrote its own line to the log saying all of this, so the log's outcome line
    /// leaves the text out instead of repeating it.
    pub logged: bool,
}

impl Reply {
    pub fn new(text: impl Into<String>, data: Json) -> Self {
        Reply { text: text.into(), data, logged: false }
    }
    pub fn text(text: impl Into<String>) -> Self {
        Reply { text: text.into(), data: Json::Null, logged: false }
    }
    pub fn logged(mut self) -> Self {
        self.logged = true;
        self
    }
}

pub fn find<'a, C>(specs: &'a [Spec<C>], name: &str) -> Option<&'a Spec<C>> {
    specs.iter().find(|s| s.name == name)
}

/// E5001 with the closest known names, so a typo is a one-step fix.
pub fn unknown<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> DawError {
    let mut close: Vec<(usize, &str)> = known.map(|k| (distance(name, k), k)).filter(|(d, _)| *d <= 3).collect();
    close.sort();
    let hint = if close.is_empty() {
        String::new()
    } else {
        format!(". Did you mean: {}?", close.iter().take(3).map(|(_, k)| *k).collect::<Vec<_>>().join(", "))
    };
    DawError::new(Code::UnknownCommand, format!("'{name}'{hint}"))
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Turn the tokens after the command name into typed arguments, checking every one.
pub fn parse<C>(spec: &Spec<C>, tokens: &[String]) -> Result<Args, DawError> {
    let mut args = Args::default();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        i += 1;
        let (arg, raw) = if let Some(name) = tok.strip_prefix("--").filter(|n| !n.is_empty()) {
            let arg = spec.args.iter().find(|a| a.name == name).ok_or_else(|| {
                bad(format!("{} has no option --{name}. Usage: {}", spec.name, usage(spec)))
            })?;
            if arg.kind == Kind::Bool && tokens.get(i).is_none_or(|t| bool_word(t).is_none()) {
                (arg, "on".to_string())
            } else {
                let v = tokens.get(i).ok_or_else(|| bad(format!("--{name} needs a value")))?;
                i += 1;
                (arg, v.clone())
            }
        } else if let Some(words) = spec.args.iter().find(|a| a.kind == Kind::Words && args.get(a.name).is_some()) {
            let joined = format!("{} {tok}", args.text(words.name).unwrap_or_default());
            args.set(words.name, Value::Text(joined));
            continue;
        } else {
            let arg = spec.args.iter().find(|a| args.get(a.name).is_none()).ok_or_else(|| {
                bad(format!("too many values for {}: '{tok}'. Usage: {}", spec.name, usage(spec)))
            })?;
            (arg, tok.clone())
        };
        if args.get(arg.name).is_some() {
            return Err(bad(format!("'{}' given twice", arg.name)));
        }
        args.set(arg.name, convert(arg, &raw)?);
    }
    if let Some(a) = spec.args.iter().find(|a| a.required && args.get(a.name).is_none()) {
        return Err(bad(format!("{} needs '{}' ({}). Usage: {}", spec.name, a.name, a.help, usage(spec))));
    }
    Ok(args)
}

fn bool_word(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn convert(arg: &Arg, raw: &str) -> Result<Value, DawError> {
    let name = arg.name;
    match arg.kind {
        Kind::Text | Kind::Track | Kind::Words => Ok(Value::Text(raw.to_string())),
        Kind::Number => {
            let t = raw.trim();
            let t = t.strip_suffix("dB").or_else(|| t.strip_suffix("db")).unwrap_or(t);
            match t.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(Value::Number(n)),
                _ => Err(bad(format!("'{name}' needs a number, got '{raw}'"))),
            }
        }
        Kind::Int => raw.trim().parse().map(Value::Int).map_err(|_| bad(format!("'{name}' needs a whole number, got '{raw}'"))),
        Kind::Bool => bool_word(raw).map(Value::Bool).ok_or_else(|| bad(format!("'{name}' needs on or off, got '{raw}'"))),
        Kind::Choice(options) => options
            .iter()
            .find(|o| o.eq_ignore_ascii_case(raw))
            .map(|o| Value::Text((*o).to_string()))
            .ok_or_else(|| bad(format!("'{name}' must be one of {}, got '{raw}'", options.join(", ")))),
    }
}

/// The one exact text form of a parsed command: name, then `--arg value` in table order.
/// Parsing it again gives the same arguments.
pub fn canonical<C>(spec: &Spec<C>, args: &Args) -> String {
    let mut s = spec.name.to_string();
    for a in spec.args {
        let Some(v) = args.get(a.name) else { continue };
        let text = match v {
            Value::Text(t) => quote(t),
            Value::Number(n) => n.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Bool(b) => (if *b { "on" } else { "off" }).to_string(),
        };
        s.push_str(&format!(" --{} {text}", a.name));
    }
    s
}

fn quote(t: &str) -> String {
    let plain = !t.is_empty()
        && !t.starts_with("--")
        && !t.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | ';'));
    if plain { t.to_string() } else { format!("\"{}\"", t.replace('"', "\"\"")) }
}

/// One-line usage, e.g. `track.gain <track> <db>`.
pub fn usage<C>(spec: &Spec<C>) -> String {
    let mut s = spec.name.to_string();
    for a in spec.args {
        let shown = match a.kind {
            Kind::Choice(o) => o.join("|"),
            _ => a.name.to_string(),
        };
        s.push(' ');
        s.push_str(&match (a.required, a.kind) {
            (true, _) => format!("<{shown}>"),
            (false, Kind::Bool) => format!("[--{}]", a.name),
            (false, Kind::Choice(_)) => format!("[--{} {shown}]", a.name),
            (false, _) => format!("[{shown}]"),
        });
    }
    s
}

/// Several lines of help for one command.
pub fn describe<C>(spec: &Spec<C>) -> String {
    let mut s = format!("{}\n  {}\n", usage(spec), spec.help);
    for a in spec.args {
        s.push_str(&format!("    {:<10} {}\n", a.name, a.help));
    }
    s
}

/// Split text into statements of tokens. Statements end at `;` or a line break. Lines whose
/// first non-blank character is `#` are comments.
pub fn split_statements(text: &str) -> Result<Vec<Vec<String>>, DawError> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let mut tokens: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut in_token = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' | '\'' => {
                    in_token = true;
                    loop {
                        match chars.next() {
                            Some('"') if c == '"' && chars.peek() == Some(&'"') => {
                                chars.next();
                                cur.push('"');
                            }
                            Some(q) if q == c => break,
                            Some(other) => cur.push(other),
                            None => return Err(bad(format!("line {}: a quote is not closed", n + 1))),
                        }
                    }
                }
                ';' => {
                    if in_token {
                        tokens.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                    if !tokens.is_empty() {
                        out.push(std::mem::take(&mut tokens));
                    }
                }
                c if c.is_whitespace() => {
                    if in_token {
                        tokens.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                }
                c => {
                    in_token = true;
                    cur.push(c);
                }
            }
        }
        if in_token {
            tokens.push(cur);
        }
        if !tokens.is_empty() {
            out.push(tokens);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx;
    fn noop(_: &mut Ctx, _: &Args) -> Result<Reply, DawError> {
        Ok(Reply::text(""))
    }
    const KINDS: &[&str] = &["audio", "instrument"];
    static SPEC: Spec<Ctx> = Spec {
        name: "track.add",
        args: &[
            opt("name", Kind::Text, "name"),
            opt("kind", Kind::Choice(KINDS), "kind"),
            opt("db", Kind::Number, "gain"),
            opt("solo", Kind::Bool, "solo"),
        ],
        help: "Add a track.",
        effect: Effect::Edit,
        run: noop,
    };

    fn toks(s: &str) -> Vec<String> {
        split_statements(s).unwrap().remove(0)
    }

    #[test]
    fn positional_named_and_flags() {
        let a = parse(&SPEC, &toks("\"Lead Synth\" --kind INSTRUMENT -6dB --solo")[..]).unwrap();
        assert_eq!(a.text("name"), Some("Lead Synth"));
        assert_eq!(a.text("kind"), Some("instrument"));
        assert_eq!(a.number("db"), Some(-6.0));
        assert_eq!(a.flag("solo"), Some(true));
        let b = parse(&SPEC, &toks("--solo off Bass")[..]).unwrap();
        assert_eq!((b.flag("solo"), b.text("name")), (Some(false), Some("Bass")));
    }

    #[test]
    fn canonical_form_parses_back_identically() {
        for line in ["\"Say \"\"hi\"\"; now\" --db 0.5", "--name '--odd' --kind audio", "\"\""] {
            let a = parse(&SPEC, &toks(line)[..]).unwrap();
            let c = canonical(&SPEC, &a);
            let mut t = toks(&c);
            t.remove(0);
            assert_eq!(parse(&SPEC, &t).unwrap(), a, "{line} -> {c}");
        }
    }

    #[test]
    fn errors_are_coded_and_helpful() {
        let e = parse(&SPEC, &toks("--kind drum")[..]).unwrap_err();
        assert!(e.code == Code::BadArgument && e.message.contains("audio, instrument"), "{e}");
        assert_eq!(parse(&SPEC, &toks("a --db loud")[..]).unwrap_err().code, Code::BadArgument);
        assert_eq!(parse(&SPEC, &toks("a b c d e")[..]).unwrap_err().code, Code::BadArgument);
        assert_eq!(parse(&SPEC, &toks("--nope 1")[..]).unwrap_err().code, Code::BadArgument);
        let u = unknown("trak.add", ["track.add", "track.remove", "undo"].into_iter());
        assert!(u.code == Code::UnknownCommand && u.message.contains("track.add"), "{u}");
    }

    #[test]
    fn statements_comments_and_paths() {
        let s = split_statements("# comment\na 1; b \"x;y\"\n  c C:\\Music\\Song ;").unwrap();
        assert_eq!(s, vec![vec!["a", "1"], vec!["b", "x;y"], vec!["c", "C:\\Music\\Song"]]);
        assert!(split_statements("a \"open").is_err());
        assert_eq!(usage(&SPEC), "track.add [name] [--kind audio|instrument] [db] [--solo]");
    }
}
