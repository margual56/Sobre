use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderKind {
    Html,
    Markdown,
    Plain,
}

pub fn looks_like_markdown(text: &str) -> bool {
    let mut headings = 0;
    let mut fences = 0;
    let mut links = 0;
    let mut emphasis = 0;
    let mut tables = 0;
    let mut numbered = 0;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fences += 1;
            continue;
        }
        let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes)
            && trimmed[hashes..].starts_with(' ')
            && trimmed.len() > hashes + 1
        {
            headings += 1;
        }
        if trimmed.starts_with('|') && trimmed.contains("---") {
            tables += 1;
        }
        if let Some(dot) = trimmed.find(". ") {
            if dot > 0 && dot <= 3 && trimmed[..dot].bytes().all(|b| b.is_ascii_digit()) {
                numbered += 1;
            }
        }
        links += count_inline_links(trimmed);
        emphasis += trimmed.matches("**").count() / 2;
        emphasis += trimmed.matches('`').count() / 2;
    }

    let kinds = [headings, fences / 2, links, emphasis, tables, numbered]
        .iter()
        .filter(|n| **n > 0)
        .count();
    let total = headings * 2 + (fences / 2) * 3 + links * 2 + emphasis + tables * 3 + numbered;
    kinds >= 2 && total >= 4
}

fn count_inline_links(line: &str) -> usize {
    let mut count = 0;
    let mut rest = line;
    while let Some(i) = rest.find("](") {
        let before = &rest[..i];
        let after = &rest[i + 2..];
        if before.contains('[') && after.contains(')') {
            count += 1;
        }
        rest = after;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_mail_is_not_markdown() {
        let text = "Hi Marcos,\n\nThanks for the update. See you on Monday.\n\n> On Tue, you wrote:\n> can we meet?\n\n* one thing\n* another\n\nBest,\nAna";
        assert!(!looks_like_markdown(text));
    }

    #[test]
    fn readme_style_text_is_markdown() {
        let text = "# Release notes\n\nWe shipped **two** things:\n\n1. A new parser\n2. See [the docs](https://example.com/docs)\n\n```\ncargo run\n```\n";
        assert!(looks_like_markdown(text));
    }

    #[test]
    fn a_single_signal_is_not_enough() {
        assert!(!looks_like_markdown("# just one heading\nand some text"));
        assert!(!looks_like_markdown(
            "Price list:\n1. apples\n2. pears\n3. plums"
        ));
    }
}
