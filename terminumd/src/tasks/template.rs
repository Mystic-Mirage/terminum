// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
//! How an agent writes a task's text (AC-145): the project's template, else this default. Each `## `
//! heading of it without "(optional)" must stand in the text of an agent's create_task; a human's text
//! is never checked.

use rusqlite::{OptionalExtension, Transaction};

use super::ops::sql;

/// What a project's template may hold: it goes into every agent's list of tools.
pub const TEMPLATE_MOST: usize = 8 << 10;

pub const DEFAULT_TEMPLATE: &str = "Write the text as Markdown with these sections, in this order. It must stand on its own: \
someone with no access to this session must be able to act on it.

## Problem or goal
What is wrong or what must exist, and why it matters. For a bug: what happens and what should happen.

## Context
What a newcomer needs: files and functions (path:line), commands, the spec's case, error text, how it was found, related tasks.

## Expected result
What is true when it is done, as checks a reviewer can run.

## Prerequisites
What must hold or be decided before work starts: access, decisions, other tasks. \"None\" if nothing.

## Steps to reproduce (optional)
For a bug: the shortest way to see it.
";

/// The template a project's agents follow.
pub fn of_project(tx: &Transaction, project: &str) -> Result<String, String> {
    let own: Option<Option<String>> = tx.query_row("SELECT task_template FROM projects WHERE id = ?1", [project], |r| r.get(0)).optional().map_err(sql)?;
    Ok(own.flatten().unwrap_or_else(|| DEFAULT_TEMPLATE.to_string()))
}

/// A template as kept: empty or the default itself means the default (none kept).
pub fn set(tx: &Transaction, project: &str, template: &str) -> Result<(), String> {
    if template.len() > TEMPLATE_MOST {
        return Err(format!("Template: at most {} KiB", TEMPLATE_MOST >> 10));
    }
    let kept = template.trim();
    let kept = (!kept.is_empty() && kept != DEFAULT_TEMPLATE.trim()).then_some(kept);
    tx.execute("UPDATE projects SET task_template = ?1 WHERE id = ?2", (kept, project)).map_err(sql)?;
    Ok(())
}

/// A heading as compared: its words, lower case, "(optional)" left out.
fn plain(heading: &str) -> String {
    heading.to_lowercase().replace("(optional)", "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The `## ` headings a text must hold.
fn required(template: &str) -> Vec<String> {
    template.lines().filter_map(|l| l.strip_prefix("## ")).filter(|h| !h.to_lowercase().contains("(optional)")).map(plain).filter(|h| !h.is_empty()).collect()
}

/// Whether an agent's text follows the template; refused, the template is quoted so the agent can.
pub fn check(text: &str, template: &str) -> Result<(), String> {
    let has: Vec<String> = text.lines().filter_map(|l| l.trim_start().strip_prefix("## ")).map(plain).collect();
    let missing: Vec<String> = required(template).into_iter().filter(|h| !has.contains(h)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!("Not kept: the text lacks the sections {}. Write it by the project's template:\n\n{template}",
                missing.iter().map(|h| format!("\"## {h}\"")).collect::<Vec<_>>().join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac145_the_default_asks_for_its_sections_but_the_optional() {
        assert_eq!(required(DEFAULT_TEMPLATE), ["problem or goal", "context", "expected result", "prerequisites"]);
        let text = "## Problem or goal\nx\n## context\ny\n##  Expected   result\nz\n## Prerequisites\nNone";
        assert!(check(text, DEFAULT_TEMPLATE).is_ok(), "case and spaces aside, in any order");
        let err = check("## Problem or goal\nx", DEFAULT_TEMPLATE).unwrap_err();
        assert!(err.starts_with("Not kept: the text lacks the sections \"## context\", \"## expected result\", \"## prerequisites\"."), "{err}");
        assert!(err.ends_with(DEFAULT_TEMPLATE), "the refusal quotes the template");
    }

    #[test]
    fn ac145_a_template_without_headings_asks_nothing() {
        assert!(check("anything", "Say what is wrong.").is_ok());
        assert!(check("", "## Why\n## How (optional)").is_err());
        assert!(check("## why", "## Why\n## How (optional)").is_ok());
    }
}
