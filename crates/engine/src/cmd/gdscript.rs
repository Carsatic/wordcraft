//! GDScript (Godot 4) tokenizer for syntax highlighting.
//!
//! Pure and allocation-light: no dependencies, no panics, byte offsets only at char boundaries.
//! [`tokenize`] returns non-overlapping `(start, end, kind)` byte ranges in text order; anything
//! it doesn't return (whitespace, operators, plain identifiers) keeps the base code colour.

/// What a highlighted stretch is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Comment,
    DocComment,
    Str,
    Number,
    Keyword,
    Literal,
    Annotation,
    Type,
    Constant,
    FuncDef,
    Call,
    Builtin,
    NodePath,
}

const KEYWORDS: &[&str] = &[
    "if", "elif", "else", "for", "while", "match", "when", "break", "continue", "pass", "return", "class", "class_name", "extends",
    "is", "in", "as", "and", "or", "not", "await", "signal", "func", "static", "const", "enum", "var", "void", "self", "super",
    "assert", "breakpoint", "preload", "yield",
];
const LITERALS: &[&str] = &["true", "false", "null", "PI", "TAU", "INF", "NAN"];
const BUILTIN_FUNCS: &[&str] = &[
    "print", "prints", "printt", "print_rich", "printerr", "push_error", "push_warning", "len", "range", "str", "load", "abs",
    "absf", "absi", "ceil", "floor", "round", "clamp", "clampf", "clampi", "lerp", "lerpf", "min", "max", "mini", "maxi", "minf",
    "maxf", "randf", "randi", "randf_range", "randi_range", "randomize", "sign", "sqrt", "pow", "sin", "cos", "tan", "deg_to_rad",
    "rad_to_deg", "move_toward", "snapped", "typeof", "is_instance_valid", "type_convert", "var_to_str", "str_to_var", "char",
    "ord", "get_stack", "inverse_lerp", "wrapf", "wrapi", "fmod", "fposmod", "posmod", "is_equal_approx", "is_zero_approx",
];
const BUILTIN_TYPES: &[&str] = &["int", "float", "bool", "void", "Variant"];

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}
fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Tokenize `src`. Ranges are byte offsets into `src`, sorted and non-overlapping.
pub fn tokenize(src: &str) -> Vec<(usize, usize, Kind)> {
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    let mut it = src.char_indices().peekable();
    // The previous significant word (for `func name`, `class name`, `signal name`).
    let mut prev_word: Option<&str> = None;
    while let Some(&(i, c)) = it.peek() {
        let rest = &src[i..];
        // Comments.
        if c == '#' {
            let end = rest.find('\n').map_or(src.len(), |n| i + n);
            let kind = if rest.starts_with("##") { Kind::DocComment } else { Kind::Comment };
            out.push((i, end, kind));
            while it.peek().is_some_and(|&(j, _)| j < end) {
                it.next();
            }
            prev_word = None;
            continue;
        }
        // Strings, with the prefixes GDScript knows: r"…", &"…", ^"…", @"…".
        let (prefix_len, quote_at) = match c {
            'r' | '&' | '^' if matches!(rest[1..].chars().next(), Some('"') | Some('\'')) => (1, i + 1),
            '"' | '\'' => (0, i),
            _ => (0, usize::MAX),
        };
        if quote_at != usize::MAX {
            let _ = prefix_len;
            let q = src[quote_at..].chars().next().unwrap_or('"');
            let after = &src[quote_at..];
            let triple = after.starts_with(&format!("{q}{q}{q}"));
            let open = if triple { 3 } else { 1 };
            let raw = c == 'r';
            let mut j = quote_at + open;
            let mut end = src.len();
            let mut chars = src[j..].char_indices();
            while let Some((k, ch)) = chars.next() {
                let at = j + k;
                if ch == '\\' && !raw {
                    chars.next();
                    continue;
                }
                if ch == '\n' && !triple {
                    end = at; // an unterminated one-line string stops at the line end
                    break;
                }
                if ch == q {
                    if triple {
                        if src[at..].starts_with(&format!("{q}{q}{q}")) {
                            end = at + 3;
                            break;
                        }
                    } else {
                        end = at + 1;
                        break;
                    }
                }
            }
            j = end;
            out.push((i, j, Kind::Str));
            while it.peek().is_some_and(|&(k, _)| k < j) {
                it.next();
            }
            prev_word = None;
            continue;
        }
        // Node paths: $Player/Sprite2D, %UniqueName, $"Quoted/Path".
        if (c == '$' || c == '%') && rest[1..].chars().next().is_some_and(|n| is_ident_start(n) || n == '"') {
            let mut end = i + 1;
            if rest[1..].starts_with('"') {
                end = rest[2..].find('"').map_or(src.len(), |n| i + 2 + n + 1);
            } else {
                for (k, ch) in rest[1..].char_indices() {
                    if is_ident_char(ch) || ch == '/' {
                        end = i + 1 + k + ch.len_utf8();
                    } else {
                        break;
                    }
                }
            }
            out.push((i, end, Kind::NodePath));
            while it.peek().is_some_and(|&(k, _)| k < end) {
                it.next();
            }
            prev_word = None;
            continue;
        }
        // Annotations: @export, @onready, @export_range(…).
        if c == '@' && rest[1..].chars().next().is_some_and(is_ident_start) {
            let mut end = i + 1;
            for (k, ch) in rest[1..].char_indices() {
                if is_ident_char(ch) {
                    end = i + 1 + k + ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push((i, end, Kind::Annotation));
            while it.peek().is_some_and(|&(k, _)| k < end) {
                it.next();
            }
            prev_word = None;
            continue;
        }
        // Numbers: 42, 1_000, 3.14, .5, 1e-3, 0xFF, 0b1010.
        if c.is_ascii_digit() || (c == '.' && rest[1..].chars().next().is_some_and(|n| n.is_ascii_digit())) {
            let b = rest.as_bytes();
            let mut e = 0;
            if rest.starts_with("0x") || rest.starts_with("0X") {
                e = 2;
                while e < b.len() && (b[e].is_ascii_hexdigit() || b[e] == b'_') {
                    e += 1;
                }
            } else if rest.starts_with("0b") || rest.starts_with("0B") {
                e = 2;
                while e < b.len() && matches!(b[e], b'0' | b'1' | b'_') {
                    e += 1;
                }
            } else {
                while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'_') {
                    e += 1;
                }
                if e < b.len() && b[e] == b'.' && !matches!(b.get(e + 1), Some(n) if n.is_ascii_alphabetic() || *n == b'_' || *n == b'.') {
                    e += 1;
                    while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'_') {
                        e += 1;
                    }
                }
                if e < b.len() && (b[e] == b'e' || b[e] == b'E') {
                    let mut x = e + 1;
                    if x < b.len() && (b[x] == b'+' || b[x] == b'-') {
                        x += 1;
                    }
                    if x < b.len() && b[x].is_ascii_digit() {
                        while x < b.len() && b[x].is_ascii_digit() {
                            x += 1;
                        }
                        e = x;
                    }
                }
            }
            out.push((i, i + e, Kind::Number));
            while it.peek().is_some_and(|&(k, _)| k < i + e) {
                it.next();
            }
            prev_word = None;
            continue;
        }
        // Identifiers and keywords.
        if is_ident_start(c) {
            let mut end = i;
            for (k, ch) in rest.char_indices() {
                if is_ident_char(ch) {
                    end = i + k + ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &src[i..end];
            let next_non_space = src[end..].chars().find(|ch| *ch != ' ' && *ch != '\t');
            let after_dot = src[..i].trim_end_matches([' ', '\t']).ends_with('.');
            let kind = if after_dot {
                // Member access: obj.method(…) is a call, obj.property is plain.
                (next_non_space == Some('(')).then_some(Kind::Call)
            } else if matches!(prev_word, Some("func") | Some("signal")) {
                Some(Kind::FuncDef)
            } else if matches!(prev_word, Some("class") | Some("class_name") | Some("extends") | Some("enum")) {
                Some(Kind::Type)
            } else if LITERALS.contains(&word) {
                Some(Kind::Literal)
            } else if KEYWORDS.contains(&word) {
                Some(Kind::Keyword)
            } else if BUILTIN_TYPES.contains(&word) {
                Some(Kind::Type)
            } else if next_non_space == Some('(') {
                // print(…), Vector2(…), my_func(…)
                if BUILTIN_FUNCS.contains(&word) {
                    Some(Kind::Builtin)
                } else if word.chars().next().is_some_and(char::is_uppercase) {
                    Some(Kind::Type)
                } else {
                    Some(Kind::Call)
                }
            } else if word.len() > 1 && word.chars().all(|ch| ch.is_uppercase() || ch.is_ascii_digit() || ch == '_') {
                Some(Kind::Constant)
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                Some(Kind::Type)
            } else {
                None
            };
            if let Some(k) = kind {
                out.push((i, end, k));
            }
            while it.peek().is_some_and(|&(k, _)| k < end) {
                it.next();
            }
            prev_word = if kind == Some(Kind::Keyword) || matches!(word, "class_name" | "extends" | "enum") { Some(word_static(word)) } else { None };
            continue;
        }
        it.next();
        if !c.is_whitespace() {
            prev_word = None;
        }
    }
    out
}

/// The keywords that name what follows them, as `'static` strs for `prev_word`.
fn word_static(w: &str) -> &'static str {
    match w {
        "func" => "func",
        "signal" => "signal",
        "class" => "class",
        "class_name" => "class_name",
        "extends" => "extends",
        "enum" => "enum",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(&str, Kind)> {
        tokenize(src).into_iter().map(|(a, b, k)| (&src[a..b], k)).collect()
    }

    #[test]
    fn highlights_a_typical_script() {
        let src = "extends CharacterBody2D\n\n@export var speed: float = 300.0\n\nfunc _physics_process(delta):\n\tvar dir = Input.get_axis(\"left\", \"right\") # move\n\tif dir != 0:\n\t\tvelocity.x = dir * speed\n\tprint(0xFF, $Sprite2D)\n";
        let t = kinds(src);
        assert!(t.contains(&("extends", Kind::Keyword)));
        assert!(t.contains(&("CharacterBody2D", Kind::Type)));
        assert!(t.contains(&("@export", Kind::Annotation)));
        assert!(t.contains(&("float", Kind::Type)));
        assert!(t.contains(&("300.0", Kind::Number)));
        assert!(t.contains(&("_physics_process", Kind::FuncDef)));
        assert!(t.contains(&("get_axis", Kind::Call)));
        assert!(t.contains(&("\"left\"", Kind::Str)));
        assert!(t.contains(&("# move", Kind::Comment)));
        assert!(t.contains(&("print", Kind::Builtin)));
        assert!(t.contains(&("0xFF", Kind::Number)));
        assert!(t.contains(&("$Sprite2D", Kind::NodePath)));
        assert!(!t.iter().any(|(s, _)| *s == "velocity" || *s == "x"));
    }

    #[test]
    fn strings_comments_and_edges() {
        assert_eq!(kinds("\"a # not a comment\""), vec![("\"a # not a comment\"", Kind::Str)]);
        assert_eq!(kinds("\"\"\"multi\nline\"\"\""), vec![("\"\"\"multi\nline\"\"\"", Kind::Str)]);
        assert_eq!(kinds("\"esc \\\" quote\""), vec![("\"esc \\\" quote\"", Kind::Str)]);
        assert_eq!(kinds("&\"idle\""), vec![("&\"idle\"", Kind::Str)]);
        assert_eq!(kinds("\"unterminated\nvar x"), vec![("\"unterminated", Kind::Str), ("var", Kind::Keyword)]);
        assert_eq!(kinds("## doc"), vec![("## doc", Kind::DocComment)]);
        assert_eq!(kinds("1_000 + 1e-3 + .5"), vec![("1_000", Kind::Number), ("1e-3", Kind::Number), (".5", Kind::Number)]);
        assert_eq!(kinds("MAX_SPEED"), vec![("MAX_SPEED", Kind::Constant)]);
        assert_eq!(kinds("signal health_changed"), vec![("signal", Kind::Keyword), ("health_changed", Kind::FuncDef)]);
    }

    #[test]
    fn never_panics_on_junk() {
        for s in ["", "\"", "'''", "$", "$\"", "@", "0x", "1e", "é\"é", "func", "#", "r\"", "\\", "日本語 = 1", "a.", "1."] {
            let t = tokenize(s);
            let mut last = 0;
            for (a, b, _) in t {
                assert!(a >= last && a < b && b <= s.len() && s.is_char_boundary(a) && s.is_char_boundary(b), "{s:?}");
                last = b;
            }
        }
    }
}
