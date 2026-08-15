//! 极简 Markdown 渲染器：AI 生成的笔记内容（标题 / 段落 / 列表 / 表格 /
//! 引用要点 / 粗斜体 / 行内代码 / 附件图片）渲染为 HTML 字符串。
//! 输入先转义再套标签，避免注入；行内格式用顺序扫描，不做嵌套解析。
//! 图片仅放行 /api/v1/attachments/ 白名单（见 parse_image_line），src 由
//! 视图经鉴权 fetch → blob → objectURL 补全。

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// 行内格式：`code` → **bold** → *italic*。
fn inline(src: &str) -> String {
    let mut s = escape_html(src);
    // code
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(pos) = rest.find('`') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        if let Some(end) = rest.find('`') {
            out.push_str("<code>");
            out.push_str(&rest[..end]);
            out.push_str("</code>");
            rest = &rest[end + 1..];
        } else {
            out.push('`');
            out.push_str(rest);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    s = out;
    // bold
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(pos) = rest.find("**") {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 2..];
        if let Some(end) = rest.find("**") {
            out.push_str("<strong>");
            out.push_str(&rest[..end]);
            out.push_str("</strong>");
            rest = &rest[end + 2..];
        } else {
            out.push_str("**");
            out.push_str(rest);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    s = out;
    // italic
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(pos) = rest.find('*') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        if let Some(end) = rest.find('*') {
            out.push_str("<em>");
            out.push_str(&rest[..end]);
            out.push_str("</em>");
            rest = &rest[end + 1..];
        } else {
            out.push('*');
            out.push_str(rest);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn heading_level(t: &str) -> Option<usize> {
    let trimmed = t.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let mut n = 0;
    for c in trimmed.chars() {
        if c == '#' {
            n += 1;
        } else {
            break;
        }
    }
    if (1..=6).contains(&n) {
        Some(n.clamp(2, 4))
    } else {
        None
    }
}

fn is_table_sep(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let body = t.trim_matches('|').trim();
    !body.is_empty()
        && body.split('|').all(|cell| {
            let c = cell.trim();
            c.starts_with(':') || c.ends_with(':') || c == "-"
        })
        && body.contains('-')
}

fn looks_like_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.matches('|').count() >= 2
}

fn render_table(lines: &[&str]) -> (String, usize) {
    // 收集连续表行（首行表头，第二行为分隔线）
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut consumed = 0usize;
    let mut started = false;
    for (idx, line) in lines.iter().enumerate() {
        let t = line.trim();
        if !started {
            if !(looks_like_table_row(t) || t.contains('|')) {
                break;
            }
            if idx + 1 < lines.len() && is_table_sep(lines[idx + 1]) {
                started = true;
                let cells: Vec<String> = t
                    .trim_matches('|')
                    .split('|')
                    .map(|c| inline(c.trim()).trim().to_string())
                    .collect();
                rows.push(cells);
                consumed += 1;
                continue;
            }
            break;
        }
        if !looks_like_table_row(t) {
            break;
        }
        let cells: Vec<String> = t
            .trim_matches('|')
            .split('|')
            .map(|c| inline(c.trim()).trim().to_string())
            .collect();
        rows.push(cells);
        consumed += 1;
    }
    let mut out = String::from("<table class=\"cheat\">");
    if let Some(head) = rows.first() {
        out.push_str("<thead><tr>");
        for c in head {
            out.push_str(&format!("<th>{}</th>", c));
        }
        out.push_str("</tr></thead>");
    }
    out.push_str("<tbody>");
    for row in rows.iter().skip(1) {
        out.push_str("<tr>");
        for c in row {
            out.push_str(&format!("<td>{}</td>", c));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    (out, consumed)
}

fn render_list(lines: &[&str], ordered: bool) -> (String, usize) {
    let tag = if ordered { "ol" } else { "ul" };
    let mut out = format!("<{}>", tag);
    let mut consumed = 0usize;
    for line in lines {
        let t = line.trim();
        let item = if ordered {
            t.split_once(". ")
                .map(|(_, r)| r.trim())
                .or_else(|| t.split_once(".	").map(|(_, r)| r.trim()))
        } else {
            t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
        };
        match item {
            Some(content) => {
                out.push_str(&format!("<li>{}</li>", inline(content)));
                consumed += 1;
            }
            None => break,
        }
    }
    out.push_str(&format!("</{}>", tag));
    (out, consumed)
}

fn render_blockquote(lines: &[&str]) -> (String, usize) {
    let mut parts = Vec::new();
    let mut consumed = 0usize;
    for line in lines {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('>') {
            parts.push(inline(rest.trim()));
            consumed += 1;
        } else if t.is_empty() {
            consumed += 1;
        } else {
            break;
        }
    }
    (
        format!("<div class=\"exam-tip\">{}</div>", parts.join("<br>")),
        consumed,
    )
}

/// 解析块级图片 `![alt](url)`，返回 (alt, 附件 id)。
/// URL 白名单仅放行 /api/v1/attachments/{id}（本服务附件，走鉴权渲染）；
/// 外链或未知前缀返回 None，调用方将其转义为纯文本段落，不渲染 img
/// （防外链追踪与不可控内容）。
fn parse_image_line(t: &str) -> Option<(String, i64)> {
    let rest = t.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let url = rest.strip_suffix(')')?;
    let id = url.strip_prefix("/api/v1/attachments/")?.parse::<i64>().ok()?;
    Some((alt.trim().to_owned(), id))
}

/// 在渲染后的 HTML 中注入批注高亮 span。锚点按 escape_html 后的文本查找
/// （渲染 HTML 中的锚点已是转义形式）；带 cursor 跟踪，跳过来源顺序中未
/// 找到的锚点，避免重复注入或跨标签误配。
pub fn html_with_annotations(html: &str, annotations: &[crate::api::Annotation]) -> String {
    let mut out = String::with_capacity(html.len() + annotations.len() * 64);
    let mut rest = html;
    for anno in annotations {
        let needle = escape_html(&anno.anchor);
        let Some(pos) = rest.find(&needle) else {
            continue;
        };
        let (head, tail) = rest.split_at(pos);
        out.push_str(head);
        out.push_str(&anno_span(anno, &needle));
        rest = &tail[needle.len()..];
    }
    out.push_str(rest);
    out
}

fn anno_span(anno: &crate::api::Annotation, escaped_anchor: &str) -> String {
    let mine = anno.author == crate::api::AnnotationAuthor::User;
    let mine_cls = if mine { " mine" } else { "" };
    let text = escape_html(&anno.text);
    let quote = escape_html(&anno.anchor);
    format!(
        "<span class=\"annotation-highlight{}\" data-anno-id=\"{}\" data-item-id=\"{}\" \
         data-quote=\"{}\" data-text=\"{}\" data-mine=\"{}\">{}<span class=\"annotation-popup\">📌 {}</span></span>",
        mine_cls,
        anno.id,
        anno.item_id,
        quote,
        text,
        mine,
        escaped_anchor,
        text,
    )
}

pub fn render_markdown(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(h) = heading_level(t) {
            let title = t.split_once('#').map(|x| x.1).unwrap_or("").trim();
            out.push_str(&format!("<h{}>{}</h{}>", h, inline(title), h));
            i += 1;
            continue;
        }
        if is_table_sep(line) {
            i += 1;
            continue; // 悬空分隔线，忽略
        }
        if looks_like_table_row(t) && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
            let (html, n) = render_table(&lines[i..]);
            out.push_str(&html);
            i += n;
            continue;
        }
        if t.starts_with("![") {
            // 白名单内：渲染占位 img（data-uid 携带附件 id，src 由视图经
            // 鉴权 fetch → blob 后补上）；白名单外落回段落按纯文本转义。
            if let Some((alt, id)) = parse_image_line(t) {
                out.push_str(&format!(
                    "<p class=\"note-image\"><img data-uid=\"{}\" alt=\"{}\" loading=\"lazy\"></p>",
                    id,
                    escape_html(&alt)
                ));
                i += 1;
                continue;
            }
        }
        if t.starts_with("- ") || t.starts_with("* ") {
            let (html, n) = render_list(&lines[i..], false);
            out.push_str(&html);
            i += n;
            continue;
        }
        if t.starts_with(">") {
            let (html, n) = render_blockquote(&lines[i..]);
            out.push_str(&html);
            i += n;
            continue;
        }
        if t.chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            if let Some((_, rest)) = t.split_once(". ") {
                if !rest.is_empty() {
                    let (html, n) = render_list(&lines[i..], true);
                    out.push_str(&html);
                    i += n;
                    continue;
                }
            }
        }
        // 段落：收集直到空行或块级起始
        let mut para = Vec::new();
        while i < lines.len() {
            let l = lines[i].trim();
            if l.is_empty()
                || heading_level(l).is_some()
                || looks_like_table_row(l)
                || l.starts_with(">")
                || l.starts_with("- ")
                || l.starts_with("* ")
                || l.starts_with("![")
            {
                break;
            }
            para.push(inline(l));
            i += 1;
        }
        if !para.is_empty() {
            out.push_str(&format!("<p>{}</p>", para.join(" ")));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_in_whitelist_renders_placeholder() {
        assert_eq!(
            render_markdown("![示意图](/api/v1/attachments/42)"),
            "<p class=\"note-image\"><img data-uid=\"42\" alt=\"示意图\" loading=\"lazy\"></p>"
        );
    }

    #[test]
    fn image_alt_is_escaped() {
        let html = render_markdown("![<script>](/api/v1/attachments/1)");
        assert!(html.contains("alt=\"&lt;script&gt;\""));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn image_outside_whitelist_is_plain_text() {
        let html = render_markdown("![外链](https://evil.example/x.png)");
        assert!(!html.contains("<img"));
        assert!(html.contains("![外链](https://evil.example/x.png)"));
    }

    #[test]
    fn image_breaks_paragraph_collection() {
        let html = render_markdown("一段文字\n![图](/api/v1/attachments/7)");
        assert!(html.contains("<p>一段文字</p>"));
        assert!(html.contains("data-uid=\"7\""));
    }
}
