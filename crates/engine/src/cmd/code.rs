//! Code snippets (Insert › Text): `code.gdscript` turns the selected paragraphs into a
//! syntax-highlighted GDScript (Godot 4) snippet.
//!
//! It only sets ordinary character and paragraph formatting (monospace font, per-token colours,
//! shading), so the result is plain .docx / PDF / ODT content and needs no new document model.
//! The tokenizer is [`super::gdscript`]. One command, one undo step.

use serde_json::{Value, json};
use wordcraft_doc::props::{Rgb, TextColor};
use wordcraft_doc::{Path, Pos};

use super::gdscript::{Kind, tokenize};
use super::{format, para};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("code.gdscript", "GDScript Snippet", "Insert › Text", gdscript)
            .params(r#"{"theme"?: "dark" (default) | "light", "font"?: string (default Consolas), "size"?: number (points, default 10)} — formats the selected paragraphs as a highlighted GDScript snippet"#),
    ]
}

/// A snippet colour scheme: block background, default text and one colour per token kind.
struct Palette {
    bg: Rgb,
    fg: Rgb,
    comment: Rgb,
    string: Rgb,
    number: Rgb,
    keyword: Rgb,
    literal: Rgb,
    annotation: Rgb,
    type_: Rgb,
    constant: Rgb,
    func_def: Rgb,
    call: Rgb,
    builtin: Rgb,
    node_path: Rgb,
}

const DARK: Palette = Palette {
    bg: Rgb(0x23, 0x29, 0x32),
    fg: Rgb(0xd5, 0xdc, 0xe6),
    comment: Rgb(0x7a, 0x86, 0x96),
    string: Rgb(0xff, 0xed, 0xa1),
    number: Rgb(0xa1, 0xff, 0xe0),
    keyword: Rgb(0xff, 0x70, 0x85),
    literal: Rgb(0xff, 0x8c, 0xcc),
    annotation: Rgb(0xff, 0xb3, 0x73),
    type_: Rgb(0x42, 0xff, 0xc2),
    constant: Rgb(0xff, 0xcf, 0x8a),
    func_def: Rgb(0x57, 0xb3, 0xff),
    call: Rgb(0x8c, 0xcf, 0xff),
    builtin: Rgb(0x57, 0xb3, 0xff),
    node_path: Rgb(0x63, 0xc2, 0x59),
};

const LIGHT: Palette = Palette {
    bg: Rgb(0xf4, 0xf6, 0xf8),
    fg: Rgb(0x24, 0x2b, 0x33),
    comment: Rgb(0x6a, 0x73, 0x7d),
    string: Rgb(0x8a, 0x5a, 0x00),
    number: Rgb(0x00, 0x7a, 0x62),
    keyword: Rgb(0xc2, 0x2a, 0x45),
    literal: Rgb(0xb0, 0x2a, 0x84),
    annotation: Rgb(0xb3, 0x58, 0x00),
    type_: Rgb(0x00, 0x7a, 0x5a),
    constant: Rgb(0x9a, 0x5b, 0x00),
    func_def: Rgb(0x0b, 0x5f, 0xb3),
    call: Rgb(0x1f, 0x6f, 0xa8),
    builtin: Rgb(0x0b, 0x5f, 0xb3),
    node_path: Rgb(0x2a, 0x7a, 0x22),
};

impl Palette {
    fn of(&self, k: Kind) -> Rgb {
        match k {
            Kind::Comment | Kind::DocComment => self.comment,
            Kind::Str => self.string,
            Kind::Number => self.number,
            Kind::Keyword => self.keyword,
            Kind::Literal => self.literal,
            Kind::Annotation => self.annotation,
            Kind::Type => self.type_,
            Kind::Constant => self.constant,
            Kind::FuncDef => self.func_def,
            Kind::Call => self.call,
            Kind::Builtin => self.builtin,
            Kind::NodePath => self.node_path,
        }
    }
}

/// One selected paragraph's slice of the snippet text.
struct Seg {
    path: Path,
    /// Byte range of the slice in the paragraph.
    from: usize,
    to: usize,
    /// Byte offset of the slice in the joined snippet text.
    start: usize,
}

fn gdscript(s: &mut Session, v: &Value) -> CmdResult {
    let pal = match p::str(v, "theme") {
        None | Some("dark") => &DARK,
        Some("light") => &LIGHT,
        Some(other) => return Err(CmdError::Params(format!("unknown theme `{other}`: use dark or light"))),
    };
    let font = match p::str(v, "font").map(str::trim) {
        None => "Consolas".to_string(),
        Some(f) if !f.is_empty() && f.len() <= 128 => f.to_string(),
        Some(_) => return Err(CmdError::Params("bad font name".into())),
    };
    let size = p::f32(v, "size").unwrap_or(10.0).clamp(6.0, 72.0);

    let (a, b) = s.sel.ordered();
    if a == b {
        return Err(CmdError::Failed("select the GDScript code first".into()));
    }

    // The selected text, one paragraph per line, remembering where each line came from.
    let mut src = String::new();
    let mut segs: Vec<Seg> = Vec::new();
    for path in s.doc.paths_between(&a, &b) {
        let Some(par) = s.doc.para(a.story, &path) else { continue };
        let from = if path == a.path { a.off } else { 0 };
        let to = if path == b.path { b.off } else { par.len() };
        let Some(text) = par.text.get(from..to) else { continue };
        if !segs.is_empty() {
            src.push('\n');
        }
        segs.push(Seg { path, from, to, start: src.len() });
        src.push_str(text);
    }
    if src.trim().is_empty() {
        return Err(CmdError::Failed("select the GDScript code first".into()));
    }

    // 1. The whole block: monospace, default colour, shaded, no leftover bold/italic.
    let (bg, fg) = (pal.bg, pal.fg);
    format::apply(s, &|c| {
        c.font = Some(font.clone());
        c.size = Some(size);
        c.color = Some(TextColor::Rgb(fg));
        c.shading = Some(bg);
        c.bold = Some(false);
        c.italic = Some(false);
    })?;
    // Paragraphs: shaded and tight, so the lines read as one block.
    para::fmt(s, &|pp| {
        pp.shading = Some(bg);
        pp.space_before = Some(0.0);
        pp.space_after = Some(0.0);
    })?;

    // 2. Colour each token; a token that spans paragraphs (a """string""") is split per paragraph.
    for (ts, te, kind) in tokenize(&src) {
        let rgb = pal.of(kind);
        let italic = matches!(kind, Kind::Comment | Kind::DocComment);
        for seg in &segs {
            let seg_end = seg.start + (seg.to - seg.from);
            let (x, y) = (ts.max(seg.start), te.min(seg_end));
            if x >= y {
                continue;
            }
            let (from, to) = (seg.from + (x - seg.start), seg.from + (y - seg.start));
            let (pa, pb) = (Pos::new(a.story, seg.path.clone(), from), Pos::new(a.story, seg.path.clone(), to));
            s.doc.format_range(&pa, &pb, &|c| {
                c.color = Some(TextColor::Rgb(rgb));
                if italic {
                    c.italic = Some(true);
                }
            })?;
        }
    }
    Ok(json!({"tokens": tokenize(&src).len(), "paragraphs": segs.len(), "theme": if std::ptr::eq(pal, &LIGHT) { "light" } else { "dark" }}))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Document, Path, StoryRef};

    use crate::Session;

    fn color_at(s: &Session, off: usize) -> Option<wordcraft_doc::props::TextColor> {
        s.doc.para(StoryRef::Body, &Path::top(0))?.props_of_char(off).color
    }

    #[test]
    fn highlights_the_selected_snippet() {
        let mut s = Session::new(Document::new());
        s.run("text.insert", &json!({"text": "var speed = 300 # fast"})).unwrap();
        s.run("select.all", &json!({})).unwrap();
        let r = s.run("code.gdscript", &json!({})).unwrap();
        assert!(r["tokens"].as_u64().unwrap() >= 3);
        // `var` is a keyword, `300` a number, `# fast` a comment: three different colours.
        let (kw, num, cm) = (color_at(&s, 0), color_at(&s, 12), color_at(&s, 17));
        assert!(kw.is_some() && num.is_some() && cm.is_some());
        assert_ne!(kw, num);
        assert_ne!(num, cm);
        // One undo step restores the plain text formatting.
        s.run("edit.undo", &json!({})).unwrap();
        assert_ne!(color_at(&s, 0), kw);
    }

    #[test]
    fn refuses_an_empty_selection_and_bad_params() {
        let mut s = Session::new(Document::new());
        s.run("text.insert", &json!({"text": "var x"})).unwrap();
        assert!(s.run("code.gdscript", &json!({})).is_err());
        s.run("select.all", &json!({})).unwrap();
        assert!(s.run("code.gdscript", &json!({"theme": "neon"})).is_err());
        assert!(s.run("code.gdscript", &json!({"font": ""})).is_err());
    }
}
