//! Blueprint templates + per-project instances — structured authoring
//! and living-state maintenance for the `AGENTS.md`/`PROJECT_SPEC.md`/
//! `PROJECT_STATE.md`/`AGENT_HANDOFF.md` bundle pattern.
//!
//! Two kinds of thing, stored differently on purpose:
//! - **Templates** (schema definitions, reusable across projects) live
//!   centrally under `Config.blueprints_dir` (`_blueprints/templates/
//!   <slug>.json`), alongside a small index of tracked project
//!   directories (`_blueprints/projects.json`).
//! - **Instances** (one per real project) store their field values
//!   *inside the target project's own directory*
//!   (`<target>/.blueprint/instance.json`) — state belongs with the code
//!   it describes, not in a separate system that can drift out of sync
//!   with what's actually in the repo.
//!
//! `PROJECT_STATE.md`/`AGENTS.md`/`PROJECT_SPEC.md` are regenerated fresh
//! from `Template + Instance` on every save (they're snapshots).
//! `AGENT_HANDOFF.md` is append-only — a new dated entry, never a full
//! rewrite, matching its real character as a running session log.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// A reusable blueprint schema (e.g. "rust-agent-blueprint"). Named
/// sections instead of one big blob, matching the real
/// `rust-agent-blueprint`'s actual `PROJECT_SPEC.md` header structure —
/// these are the parts that genuinely don't vary per project (they're
/// policy, not project-specific facts), so they're authored once here
/// and rendered into every instance built from this template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub slug: String,
    pub name: String,
    /// Full `AGENTS.md` text — mostly static operating rules, with
    /// `{{ instance.project_name }}`-style minijinja placeholders filled
    /// in per instance at render time. Kept as one block deliberately —
    /// unlike the sections below, this one really is meant to be
    /// authored close to verbatim and reused, not filled in per section.
    pub agents_md: String,
    #[serde(default)]
    pub error_policy: String,
    #[serde(default)]
    pub dependency_policy: String,
    #[serde(default)]
    pub testing_strategy: String,
    #[serde(default)]
    pub full_validation_gate: String,
    #[serde(default)]
    pub feature_isolation_gate: String,
    #[serde(default)]
    pub package_gate: String,
    #[serde(default)]
    pub ci_info: String,
    #[serde(default)]
    pub git_commit_standard: String,
    #[serde(default)]
    pub changelog_policy: String,
    #[serde(default)]
    pub release_checklist: String,
    #[serde(default)]
    pub definition_of_done: String,
}

impl Template {
    /// A real, ready-to-use starting point for a new template — these
    /// sections are genuinely boilerplate (policy, not project-specific
    /// facts), drawn from `rust-agent-blueprint`'s actual conventions, so
    /// "author a new template" means *adjust this*, not *start from
    /// nothing*.
    pub fn seed_defaults() -> Self {
        Self {
            slug: String::new(),
            name: String::new(),
            agents_md: String::new(),
            error_policy: "Use `thiserror` structured error types with source chain + context. \
No generic errors."
                .into(),
            dependency_policy: "New dependencies require: concrete need, maintenance review, \
feature review, MSRV review, platform review, security/transitive-cost review."
                .into(),
            testing_strategy: "unit (src/ inline #[cfg(test)] modules)\n\
integration (tests/*.rs)\n\
regression\n\
serde round-trip (types)\n\
compatibility (file format versioning)\n\
feature isolation (scripts/validate-features.sh)\n\
compile tests (doctests)"
                .into(),
            full_validation_gate: "cargo fmt --all --check\n\n\
git diff --check\n\n\
cargo check --workspace --all-targets --all-features\n\n\
cargo clippy --workspace --all-targets --all-features -- -D warnings\n\n\
cargo test --workspace --all-features --lib --tests\n\n\
# Doc tests must pass explicitly:\n\
cargo test --doc --workspace --all-features\n\n\
RUSTDOCFLAGS=\"-D warnings\" cargo doc --workspace --no-deps --all-features"
                .into(),
            feature_isolation_gate: "cargo check --workspace --no-default-features\n\
cargo test --workspace --no-default-features\n\n\
# Then each project-critical feature combination individually — see\n\
# this project's own Feature Matrix above for which ones."
                .into(),
            package_gate: "cargo package -p <crate> --list\n\
cargo package -p <crate>\n\n\
Package must succeed from clean, committed state. Do not use\n\
--allow-dirty as the normal fix."
                .into(),
            ci_info: "Primary CI system: GitHub Actions\n\n\
Workflow files:\n\
.github/workflows/ci.yml\n\n\
CI should cover: fmt, check, clippy, tests, docs, feature isolation,\n\
package, and a real pinned-MSRV re-check (not just static analysis)."
                .into(),
            git_commit_standard: "feat(scope): concise summary\n\n\
Why:\n\
<reason>\n\n\
Implementation:\n\
- detail\n\n\
Tests:\n\
- detail\n\n\
Validation:\n\
- command\n\n\
Compatibility:\n\
<notes>\n\n\
Commit types: feat, fix, refactor, test, docs, build, ci, perf, security, chore."
                .into(),
            changelog_policy: "Sections: Added, Changed, Deprecated, Removed, Fixed, Security."
                .into(),
            release_checklist: "[ ] Full gate green\n\
[ ] Feature isolation green\n\
[ ] Package list inspected\n\
[ ] cargo package green\n\
[ ] Cargo.lock committed if tracked\n\
[ ] CHANGELOG updated\n\
[ ] Version updated\n\
[ ] README/docs current\n\
[ ] CI green\n\
[ ] Worktree clean\n\
[ ] Release explicitly authorized"
                .into(),
            definition_of_done: "A milestone is complete only when relevant implementation, \
tests, docs, full validation, feature isolation, packaging, Git state, and handoff/state \
updates are complete."
                .into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRow {
    pub feature: String,
    pub package: String,
    pub default: bool,
    pub requires: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureStateRow {
    pub feature: String,
    pub status: String,
    pub isolation_tested: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationStatus {
    Pass,
    Fail,
    Unknown,
}

impl ValidationStatus {
    pub fn label(self) -> &'static str {
        match self {
            ValidationStatus::Pass => "PASS",
            ValidationStatus::Fail => "FAIL",
            ValidationStatus::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRow {
    pub gate: String,
    pub status: ValidationStatus,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffEntry {
    pub at: String,
    pub summary: String,
}

/// One real project's filled-in blueprint. Persisted at
/// `<target_dir>/.blueprint/instance.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub slug: String,
    pub template_slug: String,
    pub project_name: String,
    pub target_dir: PathBuf,

    // PROJECT_SPEC.md fields — Project Identity
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub msrv: String,
    #[serde(default)]
    pub workspace_members: Vec<String>,

    // PROJECT_SPEC.md fields — Vision / Requirements
    pub vision: String,
    #[serde(default)]
    pub long_term_capabilities: Vec<String>,
    pub core_requirements: Vec<String>,
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub architectural_principles: Vec<String>,
    pub feature_matrix: Vec<FeatureRow>,

    // PROJECT_SPEC.md fields — API / data / CLI contracts
    #[serde(default)]
    pub public_api_surfaces: Vec<String>,
    #[serde(default)]
    pub persistence_notes: Vec<String>,
    #[serde(default)]
    pub security_invariants: Vec<String>,
    #[serde(default)]
    pub cli_contract: String,

    pub phases: Vec<String>,
    pub protected_areas: Vec<String>,
    pub raw_idea: String,

    // PROJECT_STATE.md living fields
    pub current_phase: String,
    pub current_milestone: String,
    pub feature_state: Vec<FeatureStateRow>,
    pub validation_status: Vec<ValidationRow>,
    pub current_failure: String,
    pub next_intended_work: String,
    pub notes_for_next_agent: String,

    // AGENT_HANDOFF.md — append-only
    #[serde(default)]
    pub handoff_log: Vec<HandoffEntry>,
}

/// An entry in the central "which directories does the dashboard track"
/// index (`_blueprints/projects.json`) — not the instance data itself,
/// just enough for the list view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIndexEntry {
    pub slug: String,
    pub project_name: String,
    pub target_dir: PathBuf,
}

fn templates_dir(blueprints_dir: &Path) -> PathBuf {
    blueprints_dir.join("templates")
}

fn projects_index_path(blueprints_dir: &Path) -> PathBuf {
    blueprints_dir.join("projects.json")
}

fn instance_path(target_dir: &Path) -> PathBuf {
    target_dir.join(".blueprint").join("instance.json")
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ── Templates ────────────────────────────────────────────────────────

pub fn list_templates(blueprints_dir: &Path) -> Result<Vec<Template>> {
    let dir = templates_dir(blueprints_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let raw = fs::read_to_string(&path)?;
            out.push(
                serde_json::from_str(&raw)
                    .with_context(|| format!("parsing {}", path.display()))?,
            );
        }
    }
    out.sort_by(|a: &Template, b: &Template| a.slug.cmp(&b.slug));
    Ok(out)
}

pub fn load_template(blueprints_dir: &Path, slug: &str) -> Result<Option<Template>> {
    let path = templates_dir(blueprints_dir).join(format!("{slug}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn save_template(blueprints_dir: &Path, template: &Template) -> Result<()> {
    let dir = templates_dir(blueprints_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", template.slug));
    fs::write(&path, serde_json::to_string_pretty(template)?)
        .with_context(|| format!("writing {}", path.display()))
}

// ── Project index ───────────────────────────────────────────────────

pub fn list_tracked_projects(blueprints_dir: &Path) -> Result<Vec<ProjectIndexEntry>> {
    let path = projects_index_path(blueprints_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

fn save_tracked_projects(blueprints_dir: &Path, entries: &[ProjectIndexEntry]) -> Result<()> {
    fs::create_dir_all(blueprints_dir)?;
    let path = projects_index_path(blueprints_dir);
    fs::write(&path, serde_json::to_string_pretty(entries)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn track_project(blueprints_dir: &Path, entry: ProjectIndexEntry) -> Result<()> {
    let mut entries = list_tracked_projects(blueprints_dir)?;
    entries.retain(|e| e.slug != entry.slug);
    entries.push(entry);
    entries.sort_by(|a, b| a.project_name.cmp(&b.project_name));
    save_tracked_projects(blueprints_dir, &entries)
}

// ── Instances ────────────────────────────────────────────────────────

pub fn load_instance(target_dir: &Path) -> Result<Option<Instance>> {
    let path = instance_path(target_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

/// Load an instance by slug, by looking it up in the tracked-projects
/// index first (the index is how the dashboard finds the target
/// directory for a given slug without walking the filesystem).
pub fn load_instance_by_slug(blueprints_dir: &Path, slug: &str) -> Result<Option<Instance>> {
    let entries = list_tracked_projects(blueprints_dir)?;
    let Some(entry) = entries.into_iter().find(|e| e.slug == slug) else {
        return Ok(None);
    };
    load_instance(&entry.target_dir)
}

pub fn save_instance(blueprints_dir: &Path, instance: &Instance) -> Result<()> {
    let path = instance_path(&instance.target_dir);
    fs::create_dir_all(path.parent().expect("instance path always has a parent"))?;
    fs::write(&path, serde_json::to_string_pretty(instance)?)
        .with_context(|| format!("writing {}", path.display()))?;
    track_project(
        blueprints_dir,
        ProjectIndexEntry {
            slug: instance.slug.clone(),
            project_name: instance.project_name.clone(),
            target_dir: instance.target_dir.clone(),
        },
    )
}

pub fn append_handoff(blueprints_dir: &Path, slug: &str, summary: String) -> Result<Instance> {
    let mut instance = load_instance_by_slug(blueprints_dir, slug)?
        .with_context(|| format!("no tracked instance for slug {slug}"))?;
    instance.handoff_log.push(HandoffEntry {
        at: Utc::now().to_rfc3339(),
        summary,
    });
    save_instance(blueprints_dir, &instance)?;
    Ok(instance)
}

// ── Form encoding: one item per line, pipe-delimited for row types ─────
//
// v1 keeps the edit forms as plain textareas rather than dynamic
// add/remove-row JS — each line is one list item, or for the row types
// (Feature Matrix / Feature State / Validation Status) one
// pipe-delimited row. Still real, typed data once parsed; just a
// simpler input UX than per-field controls for now.

pub fn parse_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn format_lines(items: &[String]) -> String {
    items.join("\n")
}

pub fn parse_feature_matrix(s: &str) -> Vec<FeatureRow> {
    parse_lines(s)
        .into_iter()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(5, '|').map(str::trim).collect();
            let [feature, package, default, requires, purpose] = parts.try_into().ok()?;
            Some(FeatureRow {
                feature: feature.to_string(),
                package: package.to_string(),
                default: matches!(default.to_ascii_lowercase().as_str(), "yes" | "true" | "on"),
                requires: requires.to_string(),
                purpose: purpose.to_string(),
            })
        })
        .collect()
}

pub fn format_feature_matrix(rows: &[FeatureRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "{}|{}|{}|{}|{}",
                r.feature,
                r.package,
                if r.default { "yes" } else { "no" },
                r.requires,
                r.purpose
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_feature_state(s: &str) -> Vec<FeatureStateRow> {
    parse_lines(s)
        .into_iter()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').map(str::trim).collect();
            let [feature, status, tested, notes] = parts.try_into().ok()?;
            Some(FeatureStateRow {
                feature: feature.to_string(),
                status: status.to_string(),
                isolation_tested: matches!(
                    tested.to_ascii_lowercase().as_str(),
                    "yes" | "true" | "on"
                ),
                notes: notes.to_string(),
            })
        })
        .collect()
}

pub fn format_feature_state(rows: &[FeatureStateRow]) -> String {
    rows.iter()
        .map(|r| {
            format!(
                "{}|{}|{}|{}",
                r.feature,
                r.status,
                if r.isolation_tested { "yes" } else { "no" },
                r.notes
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_validation_status(s: &str) -> Vec<ValidationRow> {
    parse_lines(s)
        .into_iter()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').map(str::trim).collect();
            let [gate, status, notes] = parts.try_into().ok()?;
            let status = match status.to_ascii_uppercase().as_str() {
                "PASS" => ValidationStatus::Pass,
                "FAIL" => ValidationStatus::Fail,
                _ => ValidationStatus::Unknown,
            };
            Some(ValidationRow {
                gate: gate.to_string(),
                status,
                notes: notes.to_string(),
            })
        })
        .collect()
}

pub fn format_validation_status(rows: &[ValidationRow]) -> String {
    rows.iter()
        .map(|r| format!("{}|{}|{}", r.gate, r.status.label(), r.notes))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Rendering: Template + Instance -> real markdown files ──────────────

fn render(source: &str, ctx: minijinja::Value) -> Result<String> {
    let env = minijinja::Environment::new();
    env.render_str(source, ctx)
        .context("rendering blueprint template")
}

pub fn render_agents_md(template: &Template, instance: &Instance) -> Result<String> {
    render(
        &template.agents_md,
        minijinja::context! { instance => instance },
    )
}

/// `# {heading}` followed by one `- {item}` bullet per entry (kept even
/// when empty — matches this codebase's existing convention elsewhere of
/// always printing the header, never silently omitting a section).
fn section_list(out: &mut String, heading: &str, items: &[String]) {
    out.push_str(&format!("# {heading}\n\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
    out.push('\n');
}

/// `# {heading}` followed by a fenced ```text block, one item per line.
fn section_block(out: &mut String, heading: &str, body: &str) {
    out.push_str(&format!("# {heading}\n\n```text\n{}\n```\n\n", body.trim()));
}

pub fn render_project_spec(template: &Template, instance: &Instance) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!(
        "# PROJECT_SPEC.md — {}\n\n",
        instance.project_name
    ));

    out.push_str("# Project Identity\n\n");
    out.push_str(&format!("**Project Name:** {}  \n", instance.project_name));
    out.push_str(&format!("**Rust Edition:** {}  \n", instance.edition));
    out.push_str(&format!("**MSRV:** {}  \n\n", instance.msrv));
    if !instance.workspace_members.is_empty() {
        out.push_str("## Workspace Members\n\n```text\n");
        for m in &instance.workspace_members {
            out.push_str(&format!("{m}\n"));
        }
        out.push_str("```\n\n");
    }

    out.push_str("# Vision\n\n## One-Sentence Description\n\n");
    out.push_str(&instance.vision);
    out.push_str("\n\n");
    section_list(
        &mut out,
        "Long-Term Capabilities",
        &instance.long_term_capabilities,
    );
    section_list(&mut out, "Core Requirements", &instance.core_requirements);
    section_list(&mut out, "Non-Goals", &instance.non_goals);
    section_list(
        &mut out,
        "Architectural Principles",
        &instance.architectural_principles,
    );

    out.push_str("# Feature Matrix\n\n");
    out.push_str("| Feature | Package | Default | Requires | Purpose |\n|---|---|---:|---|---|\n");
    for f in &instance.feature_matrix {
        let default = if f.default { "yes" } else { "no" };
        out.push_str(&format!(
            "| `{}` | `{}` | {} | {} | {} |\n",
            f.feature, f.package, default, f.requires, f.purpose
        ));
    }
    out.push('\n');

    section_list(
        &mut out,
        "Public API Guarantees",
        &instance.public_api_surfaces,
    );
    section_list(
        &mut out,
        "Persistence / Serialization",
        &instance.persistence_notes,
    );
    section_list(
        &mut out,
        "Integrity / Security Invariants",
        &instance.security_invariants,
    );

    out.push_str("# CLI Contract\n\n");
    out.push_str(&instance.cli_contract);
    out.push_str("\n\n");

    // Template-authored sections may also use {{ instance.* }} placeholders
    // (e.g. Git Commit Standard mentioning the project name), same as
    // AGENTS.md — render each through minijinja, not just copy verbatim.
    let ctx = minijinja::context! { instance => instance };
    section_block(
        &mut out,
        "Error Policy",
        &render(&template.error_policy, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Dependency Policy",
        &render(&template.dependency_policy, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Testing Strategy",
        &render(&template.testing_strategy, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Full Validation Gate",
        &render(&template.full_validation_gate, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Feature-Isolation Gate",
        &render(&template.feature_isolation_gate, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Package Gate",
        &render(&template.package_gate, ctx.clone())?,
    );
    section_block(&mut out, "CI", &render(&template.ci_info, ctx.clone())?);
    section_block(
        &mut out,
        "Git Commit Standard",
        &render(&template.git_commit_standard, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Changelog",
        &render(&template.changelog_policy, ctx.clone())?,
    );
    section_block(
        &mut out,
        "Release Checklist",
        &render(&template.release_checklist, ctx.clone())?,
    );

    out.push_str("# Project Phases\n\n```text\n");
    for (i, p) in instance.phases.iter().enumerate() {
        out.push_str(&format!("Phase {} — {}\n", i + 1, p));
    }
    out.push_str("```\n\n# Protected Areas\n\nDo not change without an ADR:\n\n```text\n");
    for p in &instance.protected_areas {
        out.push_str(&format!("{p}\n"));
    }
    out.push_str("```\n\n");

    section_block(
        &mut out,
        "Definition of Done",
        &render(&template.definition_of_done, ctx)?,
    );

    out.push_str("# Raw Project Idea\n\n> ");
    out.push_str(&instance.raw_idea.replace('\n', "\n> "));
    out.push('\n');

    Ok(out)
}

pub fn render_project_state(instance: &Instance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# PROJECT_STATE.md — {}\n\n",
        instance.project_name
    ));
    out.push_str("# Current Phase\n\n");
    out.push_str(&instance.current_phase);
    out.push_str("\n\n# Current Milestone\n\n");
    out.push_str(&instance.current_milestone);
    out.push_str("\n\n# Feature State\n\n");
    out.push_str("| Feature | Status | Isolation Tested? | Notes |\n|---|---|---:|---|\n");
    for f in &instance.feature_state {
        let tested = if f.isolation_tested { "yes" } else { "no" };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            f.feature, f.status, tested, f.notes
        ));
    }
    out.push_str("\n# Validation Status\n\n");
    out.push_str("| Gate | Status | Notes |\n|---|---|---|\n");
    for v in &instance.validation_status {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            v.gate,
            v.status.label(),
            v.notes
        ));
    }
    out.push_str("\n# Current Failure\n\n```text\n");
    out.push_str(if instance.current_failure.is_empty() {
        "None."
    } else {
        &instance.current_failure
    });
    out.push_str("\n```\n\n# Next Intended Work\n\n");
    out.push_str(&instance.next_intended_work);
    out.push_str("\n\n# Notes for Next Agent\n\n");
    out.push_str(&instance.notes_for_next_agent);
    out.push('\n');
    out
}

pub fn render_agent_handoff(instance: &Instance) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# AGENT_HANDOFF.md — {}\n\n",
        instance.project_name
    ));
    out.push_str("Append-only session log, newest first.\n\n");
    for entry in instance.handoff_log.iter().rev() {
        out.push_str(&format!("## {}\n\n{}\n\n", entry.at, entry.summary));
    }
    out
}

/// Write all three generated files into `instance.target_dir` and
/// (if it's a git repo) commit them there. Returns the commit outcome so
/// the caller can surface a failure instead of it vanishing — an
/// arbitrary external target might not even be a git repo.
pub fn export(template: &Template, instance: &Instance) -> Result<()> {
    let dir = &instance.target_dir;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join("AGENTS.md"), render_agents_md(template, instance)?)?;
    fs::write(
        dir.join("PROJECT_SPEC.md"),
        render_project_spec(template, instance)?,
    )?;
    fs::write(dir.join("PROJECT_STATE.md"), render_project_state(instance))?;
    fs::write(dir.join("AGENT_HANDOFF.md"), render_agent_handoff(instance))?;
    crate::git::commit_result(
        dir,
        &format!("blueprint: update state — {}", instance.project_name),
    )
}
