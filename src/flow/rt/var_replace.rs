//! Variable substitution helpers shared by all runtime node flavours.
//!
//! Two notations are recognised:
//! - `` `varName` `` — the legacy plain-text notation;
//! - `<var data-var-name="varName">label</var>` — the notation emitted by the
//!   rich-text editor, where the variable is an atomic inline node. The body
//!   (label) is skipped entirely, the value is taken from `data-var-name`.

use crate::flow::rt::dto::AnswerContentType;
use crate::result::Result;

const VAR_WRAP_SYMBOL: char = '`';

const VAR_TAG_PREFIX: &str = "<var ";
const VAR_TAG_END: &str = "</var>";
const VAR_NAME_ATTR: &str = "data-var-name";

/// Find `attr="value"` inside an opening `<var ...>` tag body.
/// Returns `(value_start, value_end)` offsets into `tag`, relative to `from`.
fn find_attr_value(tag: &str, from: usize, attr: &str) -> Option<(usize, usize)> {
    let pat = format!("{}=\"", attr);
    let start = tag[from..].find(&pat)? + from + pat.len();
    let rest = &tag[start..];
    if rest.starts_with('"') {
        return None;
    }
    let end = rest.find('"')? + start;
    if end == start {
        return None;
    }
    Some((start, end))
}

/// Replace variables wrapped by `VAR_WRAP_SYMBOL`.
fn replace_wrapped_vars<F>(text: &str, out: &mut String, mut start: usize, resolve: &F) -> usize
where
    F: Fn(&str) -> Result<Option<String>>,
{
    while let Some(rel) = text[start..].find(VAR_WRAP_SYMBOL) {
        let begin = start + rel;
        out.push_str(&text[start..begin]);
        if let Some(rel) = text[begin + 1..].find(VAR_WRAP_SYMBOL) {
            let end = begin + rel + 1;
            let name = &text[begin + 1..end];
            match resolve(name) {
                Ok(Some(value)) => out.push_str(&value),
                Ok(None) => out.push_str(&text[begin..=end]),
                Err(e) => return_log_and_keep(out, &text[begin..=end], e),
            }
            start = end + 1;
        } else {
            // Unpaired opening symbol: keep it literally.
            out.push(VAR_WRAP_SYMBOL);
            start = begin + 1;
        }
    }
    start
}

/// Replace `<var data-var-name="...">label</var>` inline nodes.
fn replace_var_tags<F>(text: &str, out: &mut String, mut start: usize, resolve: &F) -> usize
where
    F: Fn(&str) -> Result<Option<String>>,
{
    while let Some(rel) = text[start..].find(VAR_TAG_PREFIX) {
        let begin = start + rel;
        out.push_str(&text[start..begin]);
        let tag_start = begin + VAR_TAG_PREFIX.len();
        match text[tag_start..].find('>') {
            Some(rel_gt) => {
                let gt = tag_start + rel_gt;
                let open_tag = &text[begin..=gt];
                let after_open = gt + 1;
                match text[after_open..].find(VAR_TAG_END) {
                    Some(rel_end) => {
                        let close_begin = after_open + rel_end;
                        let close_end = close_begin + VAR_TAG_END.len();
                        match find_attr_value(open_tag, VAR_TAG_PREFIX.len(), VAR_NAME_ATTR) {
                            Some((ns, ne)) => {
                                let name = &open_tag[ns..ne];
                                match resolve(name) {
                                    Ok(Some(value)) => out.push_str(&value),
                                    Ok(None) => {
                                        // Unknown variable: keep the label shown to end users.
                                        out.push_str(&text[after_open..close_begin]);
                                    }
                                    Err(e) => {
                                        return_log_and_keep(
                                            out,
                                            &text[after_open..close_begin],
                                            e,
                                        );
                                    }
                                }
                            }
                            None => {
                                // Malformed tag without the name attribute: keep the label.
                                out.push_str(&text[after_open..close_begin]);
                            }
                        }
                        start = close_end;
                    }
                    None => {
                        // No closing tag: keep everything literally.
                        out.push_str(&text[begin..]);
                        return text.len();
                    }
                }
            }
            None => {
                // Unterminated opening tag: keep everything literally.
                out.push_str(&text[begin..]);
                return text.len();
            }
        }
    }
    start
}

#[inline]
fn return_log_and_keep(out: &mut String, keep: &str, e: crate::result::Error) {
    log::error!("replace_vars: {e:?}");
    out.push_str(keep);
}

/// Replace all variable references (both notations) in `text`.
/// `resolve` looks a variable up by name and returns its string value.
pub(crate) fn replace_vars_with<F>(text: &str, resolve: F) -> Result<String>
where
    F: Fn(&str) -> Result<Option<String>>,
{
    let mut out = String::with_capacity(text.len().max(128));
    let mut start = 0usize;
    while start < text.len() {
        let next_tag = text[start..].find(VAR_TAG_PREFIX);
        let next_wrap = text[start..].find(VAR_WRAP_SYMBOL);
        match (next_wrap, next_tag) {
            (Some(w), t) if t.is_none_or(|tag| w < tag) => {
                start = replace_wrapped_vars(text, &mut out, start, &resolve);
            }
            (_, Some(tag)) => {
                start = replace_var_tags(text, &mut out, start, &resolve);
            }
            _ => break,
        }
    }
    out.push_str(&text[start..]);
    Ok(out)
}

/// Strip rich-text markup from a dialog text, leaving plain display text.
///
/// - `<var>` inline nodes collapse to their inner label;
/// - `` `varName` `` wrapped variables collapse to `varName`;
/// - all other tags are removed;
/// - `<br>`/`</p>` become newlines.
pub(crate) fn text_to_plain(html: &str) -> String {
    let no_wrapped = replace_wrapped_markers(html);
    let mut out = String::with_capacity(no_wrapped.len());
    let mut rest = no_wrapped;
    while let Some(pos) = rest.find('<') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if rest.starts_with(VAR_TAG_PREFIX) {
            let tag_start = VAR_TAG_PREFIX.len();
            if let Some(rel) = rest[tag_start..].find('>') {
                let after_open = tag_start + rel + 1;
                if let Some(rel_end) = rest[after_open..].find(VAR_TAG_END) {
                    let close_begin = after_open + rel_end;
                    out.push_str(&rest[after_open..close_begin]);
                    rest = &rest[close_begin + VAR_TAG_END.len()..];
                    continue;
                }
            }
        }
        // Regular (or malformed) tag: skip up to the closing '>'.
        match rest.find('>') {
            Some(rel) => rest = &rest[rel + 1..],
            None => {
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.replace("<br>", "\n")
        .replace("</p>", "\n")
        .replace('`', "")
}

/// Replace `` `name` `` with `name` so the plain conversion above can keep
/// treating the wrapped notation as ordinary text.
fn replace_wrapped_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = 0usize;
    while let Some(rel) = s[start..].find(VAR_WRAP_SYMBOL) {
        let begin = start + rel;
        out.push_str(&s[start..begin]);
        if let Some(rel) = s[begin + 1..].find(VAR_WRAP_SYMBOL) {
            let end = begin + rel + 1;
            out.push_str(&s[begin + 1..end]);
            start = end + 1;
        } else {
            out.push_str(&s[begin..]);
            return out;
        }
    }
    out.push_str(&s[start..]);
    out
}

/// Convert a dialog text to the plain form used by runtime nodes that only
/// support plain text (e.g. sub-flows exported for other channels).
pub(crate) fn dialog_text_to_plain(text: &str, text_type: &AnswerContentType) -> String {
    if matches!(text_type, AnswerContentType::TextHtml) {
        text_to_plain(text)
    } else {
        text.replace("`", "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver(name: &str) -> Result<Option<String>> {
        match name {
            "known" => Ok(Some("VALUE".to_string())),
            _ => Ok(None),
        }
    }

    #[test]
    fn wrapped_vars_replaced() {
        assert_eq!(
            replace_vars_with("a `known` b", resolver).unwrap(),
            "a VALUE b"
        );
    }

    #[test]
    fn unknown_wrapped_var_kept() {
        assert_eq!(
            replace_vars_with("a `nope` b", resolver).unwrap(),
            "a `nope` b"
        );
    }

    #[test]
    fn unpaired_symbol_kept() {
        assert_eq!(replace_vars_with("a ` b", resolver).unwrap(), "a ` b");
        assert_eq!(replace_vars_with("`", resolver).unwrap(), "`");
    }

    #[test]
    fn var_tag_replaced() {
        let t = "a <var data-var-name=\"known\" data-var-type=\"String\">known</var> b";
        assert_eq!(replace_vars_with(t, resolver).unwrap(), "a VALUE b");
    }

    #[test]
    fn unknown_var_tag_keeps_label() {
        let t = "a <var data-var-name=\"nope\">nope</var> b";
        assert_eq!(replace_vars_with(t, resolver).unwrap(), "a nope b");
    }

    #[test]
    fn malformed_var_tag_kept_literally() {
        let t = "a <var known> b";
        assert_eq!(replace_vars_with(t, resolver).unwrap(), t);
        let t2 = "a <var data-var-name=\"known\" b";
        assert_eq!(replace_vars_with(t2, resolver).unwrap(), t2);
    }

    #[test]
    fn mixed_notations() {
        let t = "`known` and <var data-var-name=\"known\">known</var>";
        assert_eq!(
            replace_vars_with(t, resolver).unwrap(),
            "VALUE and VALUE"
        );
    }

    #[test]
    fn html_to_plain() {
        let t = "<p>hi <var data-var-name=\"known\">known</var>!</p><p>bye `known`</p>";
        assert_eq!(text_to_plain(t), "hi known!\nbye known\n");
    }

    #[test]
    fn plain_type_strips_marks() {
        assert_eq!(
            dialog_text_to_plain("hi `known`", &AnswerContentType::TextPlain),
            "hi known"
        );
    }
}
