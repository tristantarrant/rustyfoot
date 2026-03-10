// Minimal Tornado-compatible template engine.
// Supports {{ var }}, {% if var %}...{% else %}...{% end %},
// {% autoescape None %} (ignored), and {% raw %}...{% end %}.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub enum TemplateError {
    Io(std::io::Error),
    Syntax(String),
    MissingVariable(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::Io(e) => write!(f, "template IO error: {}", e),
            TemplateError::Syntax(msg) => write!(f, "template syntax error: {}", msg),
            TemplateError::MissingVariable(name) => {
                write!(f, "template missing variable: {}", name)
            }
        }
    }
}

impl From<std::io::Error> for TemplateError {
    fn from(e: std::io::Error) -> Self {
        TemplateError::Io(e)
    }
}

/// A template context holding string key-value pairs.
pub type Context = HashMap<String, String>;

/// Load a template file and render it with the given context.
pub fn render_file(path: &Path, context: &Context) -> Result<String, TemplateError> {
    let source = fs::read_to_string(path)?;
    render(&source, context)
}

/// Render a template string with the given context.
pub fn render(source: &str, context: &Context) -> Result<String, TemplateError> {
    let tokens = tokenize(source)?;
    let mut output = String::with_capacity(source.len());
    eval_tokens(&tokens, context, &mut output)?;
    Ok(output)
}

#[derive(Debug)]
enum Token {
    Text(String),
    Expr(String),
    If(String, Vec<Token>, Vec<Token>),
}

fn tokenize(source: &str) -> Result<Vec<Token>, TemplateError> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    let bytes = source.as_bytes();
    let len = bytes.len();

    while pos < len {
        if pos + 1 < len && bytes[pos] == b'{' {
            if bytes[pos + 1] == b'{' {
                // Expression: {{ ... }}
                let start = pos + 2;
                let end = find_closing(source, start, "}}")
                    .ok_or_else(|| TemplateError::Syntax("unclosed {{".into()))?;
                let expr = source[start..end].trim().to_string();
                tokens.push(Token::Expr(expr));
                pos = end + 2;
            } else if bytes[pos + 1] == b'%' {
                // Block tag: {% ... %}
                let start = pos + 2;
                let end = find_closing(source, start, "%}")
                    .ok_or_else(|| TemplateError::Syntax("unclosed {%".into()))?;
                let tag = source[start..end].trim();
                pos = end + 2;

                if tag == "autoescape None" {
                    // Ignored — we don't autoescape
                    continue;
                } else if let Some(condition) = tag.strip_prefix("if ") {
                    let condition = condition.trim().to_string();
                    let (if_body, else_body, new_pos) =
                        parse_if_block(source, pos)?;
                    tokens.push(Token::If(condition, if_body, else_body));
                    pos = new_pos;
                } else if tag.starts_with("raw") {
                    // {% raw %}...{% end %}
                    if let Some(end_pos) = source[pos..].find("{% end %}") {
                        tokens.push(Token::Text(source[pos..pos + end_pos].to_string()));
                        pos = pos + end_pos + "{% end %}".len();
                    } else {
                        return Err(TemplateError::Syntax("unclosed {% raw %}".into()));
                    }
                } else if tag == "else" || tag == "end" {
                    // These are handled by parse_if_block
                    return Err(TemplateError::Syntax(format!(
                        "unexpected {{% {} %}} outside if block",
                        tag
                    )));
                } else {
                    // Unknown tag — pass through as text for forward compatibility
                    tokens.push(Token::Text(format!("{{% {} %}}", tag)));
                }
            } else {
                tokens.push(Token::Text("{".into()));
                pos += 1;
            }
        } else {
            // Plain text — scan ahead to next { or end
            let start = pos;
            while pos < len && !(pos + 1 < len && bytes[pos] == b'{' && (bytes[pos + 1] == b'{' || bytes[pos + 1] == b'%')) {
                pos += 1;
            }
            tokens.push(Token::Text(source[start..pos].to_string()));
        }
    }

    Ok(tokens)
}

fn find_closing(source: &str, from: usize, delim: &str) -> Option<usize> {
    source[from..].find(delim).map(|i| from + i)
}

fn parse_if_block(
    source: &str,
    start: usize,
) -> Result<(Vec<Token>, Vec<Token>, usize), TemplateError> {
    // We need to find the matching {% else %} and {% end %}, handling nesting.
    let mut depth = 1;
    let mut pos = start;
    // (else_tag_start, else_body_start): position of '{' in {% else %}, and after '%}'
    let mut else_range: Option<(usize, usize)> = None;
    let bytes = source.as_bytes();
    let len = bytes.len();

    while pos < len && depth > 0 {
        if pos + 1 < len && bytes[pos] == b'{' && bytes[pos + 1] == b'%' {
            let tag_open = pos; // position of '{' in {% ... %}
            let tag_start = pos + 2;
            if let Some(tag_end) = find_closing(source, tag_start, "%}") {
                let tag = source[tag_start..tag_end].trim();
                if tag.starts_with("if ") {
                    depth += 1;
                } else if tag == "end" {
                    depth -= 1;
                    if depth == 0 {
                        let end_block_end = tag_end + 2;
                        let if_source = if let Some((else_tag_start, _)) = else_range {
                            &source[start..else_tag_start]
                        } else {
                            &source[start..tag_open]
                        };
                        let else_source = if let Some((_, else_body_start)) = else_range {
                            &source[else_body_start..tag_open]
                        } else {
                            ""
                        };
                        let if_tokens = tokenize(if_source)?;
                        let else_tokens = tokenize(else_source)?;
                        return Ok((if_tokens, else_tokens, end_block_end));
                    }
                } else if tag == "else" && depth == 1 {
                    else_range = Some((tag_open, tag_end + 2));
                }
                pos = tag_end + 2;
            } else {
                return Err(TemplateError::Syntax("unclosed {% in if block".into()));
            }
        } else {
            pos += 1;
        }
    }

    Err(TemplateError::Syntax("unclosed {% if %} block".into()))
}

fn eval_tokens(
    tokens: &[Token],
    context: &Context,
    output: &mut String,
) -> Result<(), TemplateError> {
    for token in tokens {
        match token {
            Token::Text(text) => output.push_str(text),
            Token::Expr(name) => {
                let value = context.get(name).cloned().unwrap_or_default();
                output.push_str(&value);
            }
            Token::If(condition, if_body, else_body) => {
                let truthy = is_truthy(condition, context);
                let body = if truthy { if_body } else { else_body };
                eval_tokens(body, context, output)?;
            }
        }
    }
    Ok(())
}

/// Evaluate a condition: looks up the variable in context.
/// Supports simple comparisons like `var == 'value'`.
fn is_truthy(condition: &str, context: &Context) -> bool {
    // Handle "var == 'value'" or 'var == "value"'
    if let Some((lhs, rhs)) = condition.split_once("==") {
        let lhs = lhs.trim();
        let rhs = rhs.trim().trim_matches('\'').trim_matches('"');
        let val = context.get(lhs).map(|s| s.as_str()).unwrap_or("");
        return val == rhs;
    }
    if let Some((lhs, rhs)) = condition.split_once("!=") {
        let lhs = lhs.trim();
        let rhs = rhs.trim().trim_matches('\'').trim_matches('"');
        let val = context.get(lhs).map(|s| s.as_str()).unwrap_or("");
        return val != rhs;
    }

    // Simple truthiness: non-empty, non-"false", non-"0"
    let val = context.get(condition).map(|s| s.as_str()).unwrap_or("");
    !val.is_empty() && val != "false" && val != "0" && val != "False"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_substitution() {
        let mut ctx = Context::new();
        ctx.insert("name".into(), "world".into());
        let result = render("Hello {{ name }}!", &ctx).unwrap();
        assert_eq!(result, "Hello world!");
    }

    #[test]
    fn test_if_block() {
        let mut ctx = Context::new();
        ctx.insert("show".into(), "true".into());
        let tmpl = "{% if show %}visible{% end %}";
        assert_eq!(render(tmpl, &ctx).unwrap(), "visible");

        ctx.insert("show".into(), "false".into());
        assert_eq!(render(tmpl, &ctx).unwrap(), "");
    }

    #[test]
    fn test_if_else() {
        let mut ctx = Context::new();
        ctx.insert("mode".into(), "a".into());
        let tmpl = "{% if mode == 'a' %}alpha{% else %}other{% end %}";
        assert_eq!(render(tmpl, &ctx).unwrap(), "alpha");

        ctx.insert("mode".into(), "b".into());
        assert_eq!(render(tmpl, &ctx).unwrap(), "other");
    }

    #[test]
    fn test_autoescape_ignored() {
        let ctx = Context::new();
        let result = render("{% autoescape None %}hello", &ctx).unwrap();
        assert_eq!(result, "hello");
    }
}
