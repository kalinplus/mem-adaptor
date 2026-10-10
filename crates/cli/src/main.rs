//! Implements the local CLI boundary: init prepares a home, plan saves proposed work, apply records approval,
//! writes, and saves a receipt. Registered adapters handle formats while the core engine checks the execution
//! basis and read-back evidence; `home` owns home-mode configuration, satellite resolution, and receipt filing.
//! Approval is saved before engine checks; failures after writing may leave changed targets without a saved receipt.

mod home;

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use mem_adaptor_core::canonical::{CandidateOrigin, Verdict};
use mem_adaptor_core::engine::{BasisMismatch, Engine, SCHEMA_VERSION, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::{Reader, Registry};
use mem_adaptor_core::reports::{
    Disposition, PlanReport, SatelliteSpec, UnresolvedReason, Verification,
};
use mem_adaptor_core::satellite::RelocationCandidate;
use mem_adaptor_reader_chatgpt::ChatgptReader;
use mem_adaptor_reader_claude::ClaudeReader;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use mem_adaptor_writer_ump::UmpWriter;

#[derive(Parser)]
#[command(
    name = "mem-adaptor",
    version,
    about = "Local-first memory migration with verifiable reports",
    after_help = "D1 work in progress: synthetic inputs only. Signature secret detection is local; PII checks and independent conformance are still pending.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prepare a home directory: control layout, gate policy, and the home's own index and log.
    Init {
        /// Home directory to prepare; created when absent.
        #[arg(value_name = "HOME")]
        home: PathBuf,
        /// Re-ask the gate policy and rewrite .mem-adaptor/config.toml, keeping the satellite registry.
        #[arg(long)]
        force: bool,
    },
    /// Produce a migration report without changing the target.
    Plan {
        source: PathBuf,
        #[arg(long)]
        to: String,
        /// Where the plan report is written; home mode files it under <home>/.mem-adaptor/plans/ by default.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Use the preceding receipt for idempotency and deletion protection.
        #[arg(long)]
        previous_receipt: Option<PathBuf>,
        /// Explicitly select whether secret findings pass or block.
        #[arg(long, value_parser = ["pass", "block"])]
        secret_policy: Option<String>,
        /// Allow a known rule while still reporting all its findings.
        #[arg(long)]
        allow_rule: Vec<String>,
        /// Satellite to summarise into a home: a registered ID, or `new` to issue one.
        #[arg(long, value_name = "ID|new")]
        satellite: Option<String>,
        /// Display-only satellite label; defaults to the source location's name.
        #[arg(long)]
        label: Option<String>,
    },
    /// Approve, recompute, write, and verify an existing plan.
    Apply {
        plan: PathBuf,
        #[arg(long)]
        receipt: Option<PathBuf>,
        /// Explicitly approve the plan without a terminal prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Print a local, masked comparison of the two candidates behind one entry of a plan.
    ///
    /// Reports never carry bodies (DEC-1), so this read-only view is the local channel for judging
    /// a conflict before deciding it: the satellite side is the current source file behind the entry's
    /// locator, the home side is the target file the cluster names. Nothing is written anywhere.
    Show {
        /// Plan report whose conflict to inspect.
        #[arg(value_name = "PLAN")]
        plan: PathBuf,
        /// Canonical id of the record to inspect.
        #[arg(value_name = "CANONICAL_ID")]
        canonical_id: String,
    },
    /// Scan this machine's known memory locations and list candidates for a home; writes nothing.
    ///
    /// Built-in locations are the ones [source-memory-formats.md](docs/source-memory-formats.md) documents
    /// for local harness memory (Claude Code `~/.claude/projects/*/memory`, Codex `~/.codex/memories`);
    /// extra directories passed as arguments are scanned too. Output marks each candidate as a
    /// registered satellite (with its ID and label) or unregistered, with the record count the
    /// Markdown reader would see.
    Discover {
        /// Home whose satellite registry the candidates are checked against.
        #[arg(value_name = "HOME")]
        home: PathBuf,
        /// Extra source directories to scan in addition to the built-in locations.
        #[arg(value_name = "SOURCE")]
        extra: Vec<PathBuf>,
    },
    /// Summarise every registered satellite into a home: serial plan-then-apply per satellite.
    ///
    /// Each satellite's plan is generated immediately before its own apply and never batched, so a
    /// later satellite always reconciles against the shared artifacts the earlier one just wrote. A
    /// satellite that fails (any nonzero class) is reported and the run continues with the rest; the
    /// summary lists every satellite's outcome and the exit code is the most severe class seen
    /// (input/IO 1 > stale basis 4 > incomplete 3 > clean 0). Approval semantics are identical to
    /// `apply`: interactive confirmation per satellite by default, one explicit `--yes` for the whole
    /// run when non-interactive — never a stored standing consent.
    Sync {
        /// Home whose registered satellites are summarised.
        #[arg(value_name = "HOME")]
        home: PathBuf,
        /// Explicitly approve every satellite's plan without terminal prompts.
        #[arg(long)]
        yes: bool,
    },
}

/// Presents masked ordinary errors and the documented exit code for the failure class; it does not
/// recover or roll back target writes.
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!(
                "Error: {}",
                mem_adaptor_core::gate::mask(&format!("{error:#}"))
            );
            std::process::exit(exit_code_for_error(&error));
        }
    }
}

/// Maps a failure to its documented exit code without matching error text: a stale approved execution
/// basis is its own class, and everything else that reaches this point is an input, schema or I/O
/// failure (usage errors are handled by the argument parser before this runs).
fn exit_code_for_error(error: &anyhow::Error) -> i32 {
    if error.downcast_ref::<BasisMismatch>().is_some() {
        4
    } else {
        1
    }
}

/// Counts the three ways a run is not a complete success: rejected by policy, unresolved (including
/// open conflicts and unmanaged targets), and verifications that do not match. Omitted entries
/// (duplicates, already migrated, home-modified, deleted in target) are not failures.
fn failure_counts<'a>(
    dispositions: impl Iterator<Item = &'a Disposition>,
    verifications: impl Iterator<Item = Option<&'a Verification>>,
) -> (usize, usize, usize) {
    let mut rejected = 0;
    let mut unresolved = 0;
    for disposition in dispositions {
        match disposition {
            Disposition::Rejected { .. } => rejected += 1,
            Disposition::Unresolved { .. } => unresolved += 1,
            _ => {}
        }
    }
    let unverified = verifications
        .filter(|verification| matches!(verification, Some(Verification::Mismatch { .. })))
        .count();
    (rejected, unresolved, unverified)
}

/// Exit 0 only when nothing needs a human decision; otherwise 3, the class for a run that finished
/// without errors but did not complete the migration.
fn exit_code_for(failures: (usize, usize, usize)) -> i32 {
    if failures == (0, 0, 0) { 0 } else { 3 }
}

/// Reports why a run did not exit 0, what that means for the target, and what to do next.
/// `may_have_written` distinguishes an apply (which may already have written some entries) from a plan,
/// so the message never claims a target changed when the command could not have changed it.
fn print_exit_explanation(failures: (usize, usize, usize), may_have_written: bool) {
    let (rejected, unresolved, unverified) = failures;
    if failures == (0, 0, 0) {
        println!("Exit 0: nothing outstanding; omitted entries are not failures.");
        return;
    }
    let target = if may_have_written {
        "Rejected and unresolved entries were not written; the target may be partially updated."
    } else {
        "This plan wrote no target bytes."
    };
    println!(
        "Exit 3: {rejected} rejected, {unresolved} unresolved, {unverified} unverified entries.\n\
         {target}\n\
         A conflict needs a verdict, a target_unmanaged or target_modified entry needs your decision in the home,\n\
         a policy rejection needs a rule change, and an unverified entry needs its target inspected before retrying;\n\
         re-plan after acting so the new plan and approval cover the decision."
    );
}

/// Prints the plan summary and every entry a human still has to act on, without exposing source values.
fn print_plan_detail(report: &PlanReport) {
    let count = |wanted: fn(&Disposition) -> bool| {
        report
            .entries
            .iter()
            .filter(|entry| wanted(&entry.disposition))
            .count()
    };
    println!(
        "Plan summary: {} records - {} to write, {} omitted, {} rejected, {} unresolved.",
        report.entries.len(),
        count(|disposition| matches!(
            disposition,
            Disposition::Accepted | Disposition::Transformed { .. }
        )),
        count(|disposition| matches!(disposition, Disposition::Omitted { .. })),
        count(|disposition| matches!(disposition, Disposition::Rejected { .. })),
        count(|disposition| matches!(disposition, Disposition::Unresolved { .. })),
    );
    let unresolved: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| matches!(entry.disposition, Disposition::Unresolved { .. }))
        .collect();
    if !unresolved.is_empty() {
        println!("Unresolved:");
    }
    for entry in unresolved {
        let reason = match &entry.disposition {
            Disposition::Unresolved {
                reason: UnresolvedReason::Conflict { cluster_id },
            } => format!("conflict {cluster_id}"),
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetUntracked,
            } => "target entry exists without recorded history".into(),
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetModified,
            } => "target entry changed outside this tool".into(),
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetUnmanaged,
            } => "target entry left management (its envelope is gone)".into(),
            Disposition::Unresolved { reason } => format!("{reason:?}"),
            _ => unreachable!("filtered to unresolved entries"),
        };
        println!(
            "  {} ({}) in {}: {reason}",
            entry.canonical_id, entry.source_locator, entry.target
        );
    }
    for cluster in &report.conflict_clusters {
        println!("Conflict cluster {}:", cluster.cluster_id);
        for candidate in &cluster.candidates {
            let origin = match &candidate.origin {
                mem_adaptor_core::canonical::CandidateOrigin::Satellite { id, label } => {
                    format!("satellite {id} ({})", label.clone().unwrap_or_default())
                }
                mem_adaptor_core::canonical::CandidateOrigin::Home { path } => {
                    format!("home {path}")
                }
            };
            println!(
                "  {} [{}] {} content_hash={} record_hash={}",
                candidate.canonical_id,
                candidate.basis,
                origin,
                candidate.content_hash,
                candidate.record_hash
            );
        }
    }
    let rejected: Vec<_> = report
        .entries
        .iter()
        .filter(|entry| matches!(entry.disposition, Disposition::Rejected { .. }))
        .collect();
    if !rejected.is_empty() {
        println!("Rejected by policy:");
    }
    for entry in rejected {
        let Disposition::Rejected { rule } = &entry.disposition else {
            unreachable!("filtered to rejected entries")
        };
        println!(
            "  {} ({}) in {}: {rule}",
            entry.canonical_id, entry.source_locator, entry.target
        );
    }
}

/// Reports whether this run may ask the user anything: only an attached terminal without `--yes` can.
/// Keeping this decision in one place means a non-interactive run can never block on, or silently
/// answer, a prompt, and it is the single condition the CLI uses for both prompts.
fn interactive(yes: bool, terminal: bool) -> bool {
    !yes && terminal
}

/// Asks one decision per open four-rule conflict cluster, whose two candidates describe the same record.
/// A bulk answer (`s` keep satellite everywhere, `h` keep home everywhere) settles every open cluster
/// at once yet still records one verdict per cluster, bound to that cluster's id (DEC-21 B), so a bulk
/// choice can never leak into another state; `n` records nothing. `p`, an unrecognized answer, and end
/// of input fall back to per-cluster asking, which itself leaves unresolved whatever it cannot answer,
/// so the run never invents a verdict; recorded choices persist in the receipt and are reused next run
/// (DEC-6/18).
fn collect_decisions(
    clusters: &[mem_adaptor_core::reports::ConflictCluster],
    ask: &mut dyn FnMut(&str) -> Result<Option<String>>,
) -> Result<Vec<Verdict>> {
    let mut decisions = Vec::new();
    let mut open = Vec::new();
    for cluster in clusters {
        let Some(canonical_id) = cluster
            .candidates
            .first()
            .map(|candidate| candidate.canonical_id.clone())
        else {
            continue;
        };
        let same_record = cluster
            .candidates
            .iter()
            .all(|candidate| candidate.canonical_id == canonical_id);
        let bases: Vec<_> = cluster
            .candidates
            .iter()
            .map(|candidate| candidate.basis.clone())
            .collect();
        if !same_record || bases.len() != 2 {
            println!(
                "Cluster {}: {} candidates for one record; this run records no decision for it.",
                cluster.cluster_id,
                bases.len()
            );
            continue;
        }
        open.push((cluster, canonical_id, bases));
    }
    if open.is_empty() {
        return Ok(decisions);
    }
    let prompt = format!(
        "{} open conflict cluster(s): [s] keep satellite for all / [h] keep home for all / \
         [n] leave all unresolved / [p] decide each: ",
        open.len()
    );
    match ask(&prompt)?.as_deref().map(str::trim) {
        Some(bulk @ ("s" | "h")) => {
            let side = if bulk == "s" { "satellite:" } else { "home:" };
            for (cluster, canonical_id, bases) in &open {
                let Some(basis) = bases.iter().find(|basis| basis.starts_with(side)) else {
                    println!(
                        "Cluster {}: no {side} basis; left unresolved.",
                        cluster.cluster_id
                    );
                    continue;
                };
                decisions.push(Verdict::Keep {
                    cluster_id: cluster.cluster_id.clone(),
                    canonical_ids: vec![canonical_id.clone()],
                    bases: Some(vec![basis.clone()]),
                });
            }
            println!(
                "{} cluster(s) resolved by keeping the {} value.",
                decisions.len(),
                side.trim_end_matches(':')
            );
        }
        Some("n") => println!("{} cluster(s) left unresolved.", open.len()),
        _ => {
            for (cluster, canonical_id, bases) in &open {
                let prompt = format!(
                    "Cluster {} for {}: [1] keep {} [2] keep {} [3] leave unresolved: ",
                    cluster.cluster_id, canonical_id, bases[0], bases[1]
                );
                let answer = ask(&prompt)?;
                match answer.as_deref().map(str::trim) {
                    Some("1") => decisions.push(Verdict::Keep {
                        cluster_id: cluster.cluster_id.clone(),
                        canonical_ids: vec![canonical_id.clone()],
                        bases: Some(vec![bases[0].clone()]),
                    }),
                    Some("2") => decisions.push(Verdict::Keep {
                        cluster_id: cluster.cluster_id.clone(),
                        canonical_ids: vec![canonical_id.clone()],
                        bases: Some(vec![bases[1].clone()]),
                    }),
                    _ => println!("Cluster {} left unresolved.", cluster.cluster_id),
                }
            }
        }
    }
    Ok(decisions)
}

/// Runs the read-only local view behind `mem-adaptor show`: prints both candidates of one plan entry's
/// conflict (satellite source file vs home target file) with provenance and both sides' hashes, masking
/// bodies with the gate rule set. Entries without a two-candidate cluster are explained, not guessed
/// at; unreadable files are reported explicitly. Writes nothing, changes no target state, and never
/// touches the plan (DEC-1 keeps bodies out of reports; this is the local channel instead).
fn run_show(plan_path: &Path, canonical_id: &str) -> Result<i32> {
    let report: PlanReport = serde_json::from_reader(
        fs::File::open(plan_path)
            .with_context(|| format!("cannot read plan report {}", plan_path.display()))?,
    )
    .with_context(|| format!("{} is not a valid plan report", plan_path.display()))?;
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id)
        .with_context(|| format!("canonical_id {canonical_id} is not an entry of this plan"))?;
    let cluster = report.conflict_clusters.iter().find(|cluster| {
        cluster.candidates.len() == 2
            && cluster
                .candidates
                .iter()
                .all(|candidate| candidate.canonical_id == canonical_id)
    });
    let Some(cluster) = cluster else {
        println!(
            "{canonical_id} has no two candidates in this plan: it is not in an open conflict, \
             so there is nothing to compare."
        );
        println!("Disposition: {}", disposition_text(&entry.disposition));
        println!("Source locator: {}", entry.source_locator);
        return Ok(0);
    };
    println!(
        "Conflict cluster {} for {canonical_id} (bodies masked, nothing written):",
        cluster.cluster_id
    );
    let source_root = Path::new(&report.source.location);
    let target_root = report
        .targets
        .iter()
        .find(|target| target.id == entry.target)
        .map(|target| Path::new(&target.location));
    for candidate in &cluster.candidates {
        match &candidate.origin {
            CandidateOrigin::Satellite { id, label } => {
                println!(
                    "\n[satellite side] satellite {id}{}",
                    label
                        .as_deref()
                        .map(|label| format!(" (label: {label})"))
                        .unwrap_or_default()
                );
                let path = source_root.join(&entry.source_locator);
                println!("file: {}", path.display());
                print_candidate_body(&path, candidate);
            }
            CandidateOrigin::Home { path } => {
                println!("\n[home side] home file {path}");
                let Some(root) = &target_root else {
                    println!(
                        "body unavailable: target {} is not described by this plan, \
                         so the home file cannot be attributed",
                        entry.target
                    );
                    continue;
                };
                let file = root.join(path);
                println!("file: {}", file.display());
                print_candidate_body(&file, candidate);
            }
        }
    }
    Ok(0)
}

/// Prints one candidate's hashes and its masked body, or an explicit unavailability line when the
/// file cannot be read; the other side still prints, and no content is ever invented.
fn print_candidate_body(path: &Path, candidate: &mem_adaptor_core::canonical::ConflictCandidate) {
    println!("content_hash: {}", candidate.content_hash);
    println!("record_hash: {}", candidate.record_hash);
    match fs::read_to_string(path) {
        Ok(text) => {
            println!("--- body (masked) ---");
            for line in mem_adaptor_core::gate::mask(&text).lines() {
                println!("| {line}");
            }
        }
        Err(error) => println!("body unavailable: {} ({error})", path.display()),
    }
}

/// Names a disposition for `show` output without depending on report internals beyond its status.
fn disposition_text(disposition: &Disposition) -> String {
    match disposition {
        Disposition::Accepted => "accepted (to write this run)".into(),
        Disposition::Transformed { changes } => {
            format!("transformed ({} change(s))", changes.len())
        }
        Disposition::Omitted { reason } => format!("omitted ({reason:?})"),
        Disposition::Rejected { rule } => format!("rejected ({rule})"),
        Disposition::Unresolved { reason } => format!("unresolved ({reason:?})"),
    }
}

/// Plans one source into a target and files the report: resolves or issues the satellite in home
/// mode, binds the home's history and shared basis, and never writes targets. Returns the run's exit
/// code together with the filed report path, so a batch caller can feed the same freshly generated
/// plan straight into apply; behavior matches the single `plan` command exactly.
#[allow(clippy::too_many_arguments)]
fn run_plan(
    source: PathBuf,
    to: String,
    report: Option<PathBuf>,
    previous_receipt: Option<PathBuf>,
    secret_policy: Option<String>,
    allow_rule: Vec<String>,
    satellite: Option<String>,
    label: Option<String>,
) -> Result<(i32, PathBuf)> {
    let source = normalize_path(&source)?;
    let (writer, target) = to
        .split_once(':')
        .context("Target must be okf:<directory> or ump:<directory>")?;
    ensure!(
        matches!(writer, "okf" | "ump"),
        "Target must be okf:<directory> or ump:<directory>"
    );
    let target = mem_adaptor_core::writer::normalize_root(Path::new(target)).context(
        "[S5] Target path validation failed before target writes; target unchanged. Choose a regular target directory, not a linked root, and plan again",
    )?;
    // An okf target that carries a registry is a home; everything else stays a direct migration.
    let home = crate::home::detect(writer, &target)
        .map(|directory| crate::home::load(&directory))
        .transpose()?;
    // Reports default into the home's control directory (DEC-19), which lives inside the target;
    // only that directory is exempt from the inside-a-target guard, as on the apply side.
    let control = home
        .as_ref()
        .map(|home| crate::home::control_dir(&home.directory));
    let report_path = match &report {
        Some(report) => {
            let path = normalize_path(report)?;
            let in_control = control
                .as_ref()
                .is_some_and(|control| path.starts_with(control));
            ensure!(
                !path.starts_with(&source) && (!path.starts_with(&target) || in_control),
                "Report must be outside source and target directories"
            );
            Some(path)
        }
        None => None,
    };
    ensure!(
        !target.starts_with(&source) && !source.starts_with(&target),
        "Source and target directories must not overlap"
    );
    ensure!(
        report_path.is_some() || home.is_some(),
        "--report is required for a direct migration; a home files its plans under <home>/.mem-adaptor/plans/"
    );
    if home.is_none() && writer == "okf" {
        println!(
            "NOTE: {} has no .mem-adaptor/config.toml, so this is a direct migration. \
             Run `mem-adaptor init {}` to make it a home with a satellite registry.",
            target.display(),
            target.display()
        );
    }
    let engine = engine(&[("home".into(), writer.into(), target.clone())])?;
    let interactive = io::stdin().is_terminal();
    let secret_action = secret_policy.as_deref().map(|action| {
        if action == "block" {
            GateAction::Block
        } else {
            GateAction::Pass
        }
    });
    let previous_receipt = previous_receipt
        .as_deref()
        .map(normalize_path)
        .transpose()?;
    let mut ask = read_line as fn(&str) -> Result<String>;
    let mut refuse =
        crate::home::refuse_relocation as fn(&[RelocationCandidate]) -> Result<Option<String>>;
    let (policy, spec) = match &home {
        Some(home) => {
            // The home's own configuration is the policy of record; explicit flags override this run
            // only and are marked as a user choice, exactly as in direct mode (DEC-1).
            let policy = crate::home::override_policy(
                home.config.gate_policy.clone(),
                secret_action,
                allow_rule,
            );
            let mut choose = |suspects: &[RelocationCandidate]| {
                crate::home::prompt_relocation(suspects, &mut read_answer)
            };
            let prompt: &mut dyn FnMut(&[RelocationCandidate]) -> Result<Option<String>> =
                if interactive {
                    &mut choose
                } else {
                    &mut refuse
                };
            let resolved = crate::home::resolve_satellite(
                home,
                &engine.registry,
                &source,
                satellite.as_deref(),
                label.as_deref(),
                prompt,
            )
            .context("Satellite resolution failed before target writes; target unchanged and the registry was not written")?;
            println!(
                "Satellite: {} ({}){}.",
                resolved.spec.id,
                resolved.spec.label.clone().unwrap_or_default(),
                if resolved.issues {
                    ", issued here and registered after an approved apply"
                } else {
                    ""
                }
            );
            if resolved.rebind.is_some() {
                println!(
                    "Satellite {} is bound to another path; this path replaces that binding after an approved apply.",
                    resolved.spec.id
                );
            }
            (policy, Some(resolved.spec))
        }
        None => {
            let (policy, saved) = crate::home::direct_policy(
                crate::home::user_config_path(),
                secret_action,
                allow_rule,
                interactive,
                &mut ask,
            )?;
            if let Some(path) = saved {
                println!("Saved the gate policy to {}.", path.display());
            }
            (
                policy,
                direct_satellite(satellite.as_deref(), label.as_deref())?,
            )
        }
    };
    let (report, report_path) = {
        // Home mode takes its history from the home: the satellite's own newest receipt when no
        // explicit --previous was given, plus the shared-artifact basis across all satellites
        // (DEC-18 chains, DEC-19 shared products). Direct mode keeps explicit-only history.
        let previous = match (&home, &spec) {
            (Some(home), Some(spec)) => match &previous_receipt {
                Some(path) => Some(path.clone()),
                None => crate::home::satellite_previous(home, spec.id.as_str(), "home")?,
            },
            _ => previous_receipt.clone(),
        };
        let shared_basis = match &home {
            Some(home) => crate::home::latest_shared_basis(home, "home", writer, &target)?,
            None => None,
        };
        let report = match &spec {
            Some(spec) => engine.plan_with_satellite(
                &source,
                policy,
                spec,
                previous.as_deref(),
                shared_basis.as_deref(),
            ),
            None => engine.plan_with_previous(&source, policy, previous.as_deref()),
        }
        .context("Planning failed before target writes; target unchanged. Check the source, policy and any explicitly supplied previous receipt before planning again")?;
        let path = match &report_path {
            Some(path) => path.clone(),
            None => {
                let home = home.as_ref().expect("home mode owns the default plan path");
                let satellite = spec.as_ref().expect("home mode requires a satellite");
                let path = crate::home::plan_path(home, satellite.id.as_str(), &report.run_id);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                path
            }
        };
        (report, path)
    };
    write_json_new(&report_path, &report)?;
    println!("Plan: {} records; target unchanged.", report.entries.len());
    println!("Plan filed: {}", report_path.display());
    print_gate_summary(
        &report.gate_policy,
        report
            .entries
            .iter()
            .flat_map(|entry| entry.sensitive_findings.iter()),
    );
    for warning in &report.warnings {
        println!("WARNING: {warning}");
    }
    println!("WARNING: synthetic inputs only; real-data conformance is pending.");
    print_plan_detail(&report);
    let failures = failure_counts(
        report.entries.iter().map(|entry| &entry.disposition),
        report.entries.iter().map(|_| None),
    );
    print_exit_explanation(failures, false);
    Ok((exit_code_for(failures), report_path))
}

/// Approves, recomputes, writes, and verifies one filed plan: the whole `apply` command as a
/// function so a batch caller can execute each freshly generated plan exactly once, with identical
/// guards, prompts, receipt filing, and exit-code semantics.
fn run_apply(plan: PathBuf, receipt: Option<PathBuf>, yes: bool) -> Result<i32> {
    let plan_path = normalize_path(&plan)?;
    let report: PlanReport = (|| -> Result<_> {
        serde_json::from_slice(&fs::read(&plan_path)?)
            .map_err(|_| anyhow::anyhow!("Invalid plan report JSON or fields"))
    })().context("[S7] Plan loading failed before target writes and before approval was saved; targets unchanged. Restore a valid plan or create a new one before proceeding")?;
    ensure!(
        report.schema_version == SCHEMA_VERSION,
        "Unsupported plan schema version"
    );
    let targets: Vec<_> = report
        .targets
        .iter()
        .map(|target| {
            ensure!(
                matches!(target.writer.as_str(), "okf" | "ump"),
                "Unsupported target Writer"
            );
            Ok((
                target.id.clone(),
                target.writer.clone(),
                // Keep the approved path literal: resolving a newly inserted parent link could approve another destination.
                PathBuf::from(&target.location),
            ))
        })
        .collect::<Result<_>>()?;
    // Constructor normalization must not change the destination serialized in the approved plan.
    let engine = engine(&targets).context(
        "[S7] Target setup failed before target writes and before approval was saved; this execution has not written targets. Inspect target paths and make a new plan",
    )?;
    // The plan's targets, home nature and satellite binding are all part of the approved execution
    // basis, so a drift here is the same class as the engine's own basis checks (exit 4).
    if !report.targets.iter().all(|target| {
        engine
            .registry
            .writer(&target.id)
            .is_ok_and(|writer| writer.location().to_string_lossy() == target.location)
    }) {
        return Err(anyhow::Error::new(BasisMismatch::new(
            "[S7] Approved target path changed before target writes and before approval was saved; inspect target paths and make a new plan",
        )));
    }
    let spec = report.source.satellite.clone();
    let mut home = report
        .targets
        .iter()
        .find_map(|target| crate::home::detect(&target.writer, Path::new(&target.location)))
        .map(|directory| crate::home::load(&directory))
        .transpose()?;
    if !(home.is_none() || spec.is_some()) {
        return Err(anyhow::Error::new(BasisMismatch::new(
            "[S7] This plan carries no satellite identity but its target is now a home; home mode requires a satellite. Plan again against that home",
        )));
    }
    if !(home.is_some()
        || spec.is_none()
        || !report.targets.iter().any(|target| target.writer == "okf"))
    {
        return Err(anyhow::Error::new(BasisMismatch::new(
            "[S7] This plan carries a satellite identity but its okf target has no .mem-adaptor/config.toml; the home this plan was made against is missing. Restore the home configuration or plan again; applying it as a direct migration would strand this round's receipt outside the satellite's chain",
        )));
    }
    if let (Some(home), Some(spec)) = (&home, &spec) {
        // A registry binding that no longer matches the approved plan is also a stale basis.
        crate::home::validate_plan_satellite(home, &engine.registry, spec, Path::new(&report.source.location))
            .map_err(|error| {
                anyhow::Error::new(BasisMismatch::new(format!(
                    "[S7] Plan satellite validation failed before target writes and before approval was saved; this execution has not written targets. Inspect the home registry and the plan's source before proceeding: {error:#}"
                )))
            })?;
    }
    // Home mode files the receipt in the satellite's chain inside the home (DEC-19) after the engine
    // returns it; an explicit --receipt overrides that location and keeps the original path guards.
    let receipt_path = match &receipt {
        Some(path) => Some(normalize_path(path)?),
        None if home.is_some() => None,
        None => Some(normalize_path(&plan_path.with_extension("receipt.json"))?),
    };
    if home.is_some() && receipt_path.is_some() {
        println!(
            "WARNING: --receipt overrides the home receipt chain, so relocation detection will not see this run."
        );
    }
    let approval_path = plan_path.with_extension("approval.json");
    // Reports default into the home's control directory (DEC-19), which lives inside the target;
    // only that directory is exempt from the inside-a-target guard.
    let control = home
        .as_ref()
        .map(|home| crate::home::control_dir(&home.directory));
    for path in [&approval_path].into_iter().chain(receipt_path.as_ref()) {
        ensure!(!path.exists(), "Output report already exists");
        ensure!(
            !path.starts_with(Path::new(&report.source.location)),
            "Report cannot be inside the source"
        );
        ensure!(
            targets.iter().all(|(_, _, target)| {
                !path.starts_with(target)
                    || control
                        .as_ref()
                        .is_some_and(|control| path.starts_with(control))
            }),
            "Report cannot be inside a target"
        );
    }
    ensure!(
        receipt_path.as_ref() != Some(&approval_path) && receipt_path.as_ref() != Some(&plan_path),
        "Report paths must be distinct"
    );
    if !yes {
        ensure!(
            io::stdin().is_terminal(),
            "Noninteractive apply requires explicit --yes"
        );
        print_plan_detail(&report);
    }
    // Interactive runs settle open conflicts here: the choices are recorded in this run's receipt
    // and reused next run, and because they change what a later plan proposes, the write step for
    // those entries still requires a new plan and a new approval (DEC-3, DEC-21 C).
    let decisions = if interactive(yes, io::stdin().is_terminal()) {
        let mut ask = read_answer;
        collect_decisions(&report.conflict_clusters, &mut ask)?
    } else {
        Vec::new()
    };
    if interactive(yes, io::stdin().is_terminal()) {
        print!(
            "Approve {} records for writing? [y/N] ",
            report.entries.len()
        );
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        ensure!(
            matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
            "Migration not approved"
        );
    }
    let approval = ApprovalReceipt {
        schema_version: SCHEMA_VERSION.into(),
        receipt_id: format!("approval-{}", report.run_id),
        plan_digest: report.plan_digest.clone(),
        approved_at: timestamp()?,
        backend: "local".into(),
        approver: "local-user".into(),
    };
    // Record the user's approval of this basis, even if subsequent recomputation refuses execution.
    write_json_new(&approval_path, &approval).context(
        "[S7] Approval save failed before target writes; targets unchanged, approval may be incomplete. Inspect the report path and create a new plan before proceeding; do not overwrite existing artifacts",
    )?;
    let receipt = engine.apply_with_decisions(
        &report,
        &approval,
        plan_path.to_string_lossy().into_owned(),
        approval_path.to_string_lossy().into_owned(),
        &decisions,
    ).context("Apply failed; approval was saved but no reliable final receipt was saved. Follow the failed engine stage and inspect the target state before proceeding; do not blindly retry")?;
    // Engine execution is complete; registry convergence and receipt persistence can still fail after
    // target writes, so each step reports that the target may already have changed.
    if let Some(home) = &mut home {
        crate::home::converge_registry(
            home,
            &engine.registry,
            spec.as_ref().expect("home mode requires a satellite"),
            Path::new(&report.source.location),
        )
        .context(
            "[S9] Home registry convergence failed after engine execution; targets may already have changed and approval was saved, and the satellite may be unregistered. Inspect the home registry before deciding how to proceed; do not blindly retry",
        )?;
    }
    match &receipt_path {
        Some(path) => write_json_new(path, &receipt).context(
            "[S9] Final receipt save failed after engine execution; targets may already have changed and approval was saved. No reliable final receipt was saved. Inspect targets and report paths before deciding how to proceed; do not blindly retry",
        )?,
        None => {
            let saved = crate::home::file_receipts(
                home.as_ref().expect("home mode requires a home"),
                std::slice::from_ref(&receipt),
            )
            .context(
                "[S9] Home receipt save failed after engine execution and after registry convergence; targets may already have changed and approval was saved. Inspect the home receipts directory before deciding how to proceed; do not blindly retry",
            )?;
            for path in saved {
                println!("Receipt filed: {}", path.display());
            }
        }
    }
    println!("Receipt: {} records.", receipt.entries.len());
    if !decisions.is_empty() {
        println!(
            "{} cluster decisions were recorded in this receipt; they change what the next plan proposes, so re-run plan and apply to write the decided values.",
            decisions.len()
        );
    }
    print_gate_summary(
        &receipt.gate_policy,
        receipt
            .entries
            .iter()
            .flat_map(|entry| entry.sensitive_findings.iter()),
    );
    let failures = failure_counts(
        receipt.entries.iter().map(|entry| &entry.disposition),
        receipt
            .entries
            .iter()
            .map(|entry| entry.verification.as_ref()),
    );
    print_exit_explanation(failures, true);
    Ok(exit_code_for(failures))
}

/// Scans the built-in local memory locations plus caller-given directories and lists each candidate
/// for a home: its record count as the Markdown reader sees it, and whether the home's registry already
/// binds it. Read-only end to end: nothing in the home or any scanned directory changes, so it is safe
/// to run at any time as the first step of deciding what to summarise (#47).
fn run_discover(home_directory: &Path, extra: &[PathBuf]) -> Result<i32> {
    let home = load_home(home_directory)?;
    let entries = home.config.satellites.clone().unwrap_or_default();
    let mut candidates: Vec<PathBuf> = Vec::new();
    let user_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot locate the built-in memory directories")?;
    // Claude Code nests each project's memory under ~/.claude/projects/<slug>/memory; Codex keeps its
    // memories directly under $CODEX_HOME/memories (default ~/.codex/memories;
    // docs/source-memory-formats.md).
    let claude_projects = user_home.join(".claude/projects");
    if claude_projects.is_dir() {
        match fs::read_dir(&claude_projects) {
            Ok(children) => {
                for child in children.flatten() {
                    consider_candidate(&mut candidates, &child.path().join("memory"));
                }
            }
            Err(error) => {
                println!("Unreadable: {} ({error})", claude_projects.display())
            }
        }
    }
    let codex_root = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home.join(".codex"));
    consider_candidate(&mut candidates, &codex_root.join("memories"));
    for path in extra {
        let path = normalize_path(path)?;
        if path.is_dir() {
            candidates.push(path);
        } else {
            println!("Skipped {}: not an existing directory.", path.display());
        }
    }
    candidates.sort();
    candidates.dedup();
    let reader = MarkdownReader;
    let mut registered = 0;
    let mut unregistered = 0;
    println!("Candidates for {}:", home.directory.display());
    for candidate in &candidates {
        // The registry binds canonical path strings, so compare the canonical form (read-only lookup).
        let canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone());
        let binding =
            mem_adaptor_core::satellite::find_by_path(&entries, &canonical.to_string_lossy());
        let records = (|| -> Result<String> {
            let source = mem_adaptor_core::source::load_source(&canonical)?;
            let claims = reader.claim(&source.files);
            let mut count = 0usize;
            for claim in &claims {
                count += reader.read(claim, &source)?.records.len();
            }
            Ok(format!("{count} records"))
        })()
        .unwrap_or_else(|error: anyhow::Error| format!("unreadable: {error:#}"));
        match binding {
            Some(entry) => {
                registered += 1;
                println!(
                    "  [satellite {} ({})] {} — {records}",
                    entry.id,
                    entry.label,
                    candidate.display()
                );
            }
            None => {
                unregistered += 1;
                println!("  [unregistered] {} — {records}", candidate.display());
            }
        }
    }
    println!(
        "{} candidate(s): {registered} registered, {unregistered} unregistered; nothing was written.",
        candidates.len()
    );
    Ok(0)
}

/// Adds a memory directory to the candidate list when it exists and is nonempty. An empty directory
/// is the harness's empty slot and is skipped without noise, but an unreadable one is reported
/// explicitly, matching the unreadable-source line in the listing itself.
fn consider_candidate(candidates: &mut Vec<PathBuf>, directory: &Path) {
    if !directory.is_dir() {
        return;
    }
    match fs::read_dir(directory).map(|list| list.filter_map(Result::ok).count()) {
        Ok(count) if count > 0 => candidates.push(directory.to_path_buf()),
        Ok(_) => {}
        Err(error) => println!("Unreadable: {} ({error})", directory.display()),
    }
}

/// Loads a directory as a home or fails with the fix-it message; discovery and sync both refuse to
/// guess at a directory that is not one.
fn load_home(home_directory: &Path) -> Result<crate::home::Home> {
    let directory = normalize_path(home_directory)?;
    let Some(found) = crate::home::detect("okf", &directory) else {
        anyhow::bail!(
            "{} has no .mem-adaptor/config.toml; run `mem-adaptor init {}` first",
            directory.display(),
            directory.display()
        );
    };
    crate::home::load(&found)
}

/// Summarises every registered directory satellite into a home: serial plan-then-apply per satellite,
/// each plan consumed by its own apply before the next plan is generated (#47). A satellite that fails
/// any class is reported and the loop continues, because satellites are independent — a failing one has
/// already refused safely, and the next plan reconciles against whatever the earlier applies wrote.
fn run_sync(home_directory: &Path, yes: bool) -> Result<i32> {
    let home = load_home(home_directory)?;
    let target = format!("okf:{}", home.directory.display());
    let entries = home.config.satellites.clone().unwrap_or_default();
    let directory_bound: Vec<_> = entries
        .iter()
        .filter(|entry| entry.path.is_some())
        .collect();
    for entry in entries.iter().filter(|entry| entry.path.is_none()) {
        println!(
            "Satellite {} ({}): no bound source path; skipped.",
            entry.id, entry.label
        );
    }
    if directory_bound.is_empty() {
        println!("No registered satellites to summarise.");
        return Ok(0);
    }
    let mut codes = Vec::new();
    for entry in &directory_bound {
        let source = PathBuf::from(entry.path.as_ref().expect("filtered to bound paths"));
        println!(
            "\n=== Satellite {} ({}) : {} ===",
            entry.id,
            entry.label,
            source.display()
        );
        let code = match run_plan(
            source.clone(),
            target.clone(),
            None,
            None,
            None,
            Vec::new(),
            Some(entry.id.clone()),
            None,
        ) {
            Ok((_plan_exit, plan_path)) => match run_apply(plan_path, None, yes) {
                Ok(apply_exit) => apply_exit,
                Err(error) => {
                    report_sync_error(&error);
                    exit_code_for_error(&error)
                }
            },
            Err(error) => {
                report_sync_error(&error);
                exit_code_for_error(&error)
            }
        };
        println!("Satellite {} result: exit {code}.", entry.id);
        codes.push(code);
    }
    println!("\nSync summary: {} satellite(s).", codes.len());
    for (entry, code) in directory_bound.iter().zip(&codes) {
        println!("  {} ({}): exit {code}", entry.id, entry.label);
    }
    let overall = aggregate_sync_exit(&codes);
    println!("Sync exit {overall}.");
    Ok(overall)
}

/// Prints one satellite's failure for the sync log the same way `main` presents errors: masked, with
/// the chain of stage contexts, so a failed satellite is diagnosable from the sync output alone.
fn report_sync_error(error: &anyhow::Error) {
    eprintln!(
        "Error: {}",
        mem_adaptor_core::gate::mask(&format!("{error:#}"))
    );
}

/// Aggregates per-satellite exit codes into one sync exit code: the most severe class seen, where an
/// input/IO failure (1) outweighs a stale basis (4), which outweighs an incomplete run (3). Usage
/// errors never reach this function, and clean runs stay 0.
fn aggregate_sync_exit(codes: &[i32]) -> i32 {
    for code in [1, 4, 3] {
        if codes.contains(&code) {
            return code;
        }
    }
    0
}

/// Validates CLI paths/options and orchestrates init, plan, apply, discovery, and sync.
/// Saves approval before engine recomputation and the receipt after engine completion; any error propagates.
/// A saved approval is not a success receipt, and a later receipt-save error does not imply an unchanged target.
fn run() -> Result<i32> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("LOG_LEVEL")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(io::stderr)
        .with_ansi(false)
        .without_time()
        .init();
    match cli.command {
        Command::Init { home, force } => {
            let directory = normalize_path(&home)?;
            let mut ask = read_line as fn(&str) -> Result<String>;
            let outcome =
                crate::home::init(&directory, force, io::stdin().is_terminal(), &mut ask)?;
            println!("Home: {}", directory.display());
            println!(
                "WARNING: pushing this home to a public remote publishes every memory in it. \
                 mem-adaptor does not run git init or configure remotes for you."
            );
            for path in &outcome.created {
                println!("Created:   {}", path.display());
            }
            for path in &outcome.preserved {
                println!("Preserved: {}", path.display());
            }
            println!(
                "Secret policy: {} ({}).",
                action_name(outcome.secrets),
                origin_name(outcome.origin)
            );
            if outcome.rewrote_config {
                println!(
                    "--force rewrote {} and kept the existing satellite registry.",
                    mem_adaptor_core::satellite::home_config_path(&directory).display()
                );
            }
            Ok(0)
        }
        Command::Plan {
            source,
            to,
            report,
            previous_receipt,
            secret_policy,
            allow_rule,
            satellite,
            label,
        } => run_plan(
            source,
            to,
            report,
            previous_receipt,
            secret_policy,
            allow_rule,
            satellite,
            label,
        )
        .map(|(code, _filed)| code),
        Command::Apply { plan, receipt, yes } => run_apply(plan, receipt, yes),
        Command::Show { plan, canonical_id } => {
            let plan_path = normalize_path(&plan)?;
            run_show(&plan_path, &canonical_id)
        }
        Command::Discover { home, extra } => run_discover(&home, &extra),
        Command::Sync { home, yes } => run_sync(&home, yes),
    }
}

/// Builds a direct-mode satellite identity from explicit flags. Direct mode has no registry, so it can neither
/// issue `new` nor check an ID against registrations; the engine still validates the ID shape and label.
fn direct_satellite(satellite: Option<&str>, label: Option<&str>) -> Result<Option<SatelliteSpec>> {
    match satellite {
        None => {
            ensure!(label.is_none(), "--label requires --satellite");
            Ok(None)
        }
        Some("new") => bail!(
            "--satellite new needs a home registry to issue into. Run `mem-adaptor init <home>` and target it with --to okf:<home>, or pass an explicit 8-character satellite ID"
        ),
        Some(id) => Ok(Some(SatelliteSpec {
            id: id.into(),
            label: label.map(str::to_owned),
        })),
    }
}

/// Prints a prompt and reads one answer line; end of input yields no answer at all, which callers must treat
/// as a missing choice instead of an empty one.
fn read_answer(prompt: &str) -> Result<Option<String>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    Ok((io::stdin().read_line(&mut answer)? > 0).then_some(answer))
}

/// Prints a prompt and reads one answer line; end of input yields an empty answer.
fn read_line(prompt: &str) -> Result<String> {
    Ok(read_answer(prompt)?.unwrap_or_default())
}

/// Names a gate action for user-facing output.
fn action_name(action: GateAction) -> &'static str {
    match action {
        GateAction::Pass => "pass",
        GateAction::Block => "block",
    }
}

/// Names a policy provenance so a default is never presented as a user choice.
fn origin_name(origin: PolicyOrigin) -> &'static str {
    match origin {
        PolicyOrigin::UserChoice => "user choice",
        PolicyOrigin::Default => "shipped default, not a user choice",
    }
}

/// Registers local Readers and already selected target Writers; core code remains adapter-independent.
fn engine(targets: &[(String, String, PathBuf)]) -> Result<Engine> {
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader)?;
    registry.register_reader(ChatgptReader)?;
    registry.register_reader(ClaudeReader)?;
    for (id, writer, path) in targets {
        match writer.as_str() {
            "okf" => registry.register_writer(id.clone(), OkfWriter::new(path.clone())?)?,
            "ump" => registry.register_writer(id.clone(), UmpWriter::new(path.clone())?)?,
            _ => unreachable!(),
        }
    }
    Ok(Engine { registry })
}

/// Prints finding counts and policy provenance without exposing detected values or claiming full PII coverage.
fn print_gate_summary<'a>(policy: &GatePolicy, findings: impl Iterator<Item = &'a Finding>) {
    let findings: Vec<_> = findings.collect();
    let blocked = findings
        .iter()
        .filter(|finding| finding.disposition == FindingDisposition::Blocked)
        .count();
    let allowlisted = findings
        .iter()
        .filter(|finding| finding.disposition == FindingDisposition::Allowlisted)
        .count();
    println!(
        "Secrets: {} findings, {blocked} blocked, {allowlisted} allowlisted; detection always on.",
        findings.len()
    );
    if !policy.user_selected {
        println!("Policy comes from defaults, not a user choice.");
    }
}

/// Resolves existing path prefixes before overlap checks, retaining a not-yet-created suffix.
fn normalize_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(
            existing
                .file_name()
                .context("Invalid filesystem path")?
                .to_owned(),
        );
        existing = existing.parent().context("Invalid filesystem path")?;
    }
    let mut normalized = fs::canonicalize(existing)?;
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_adaptor_core::reports::{ConflictCluster, OmissionReason};

    /// A stale approved basis is its own exit class, even when the caller adds context around it.
    #[test]
    fn basis_mismatch_errors_map_to_exit_four() {
        let mismatched = anyhow::Error::new(BasisMismatch::new("Plan digest mismatch"))
            .context("Apply failed; approval was saved");
        assert_eq!(exit_code_for_error(&mismatched), 4);
        assert_eq!(
            exit_code_for_error(&anyhow::anyhow!("Input is unreadable")),
            1
        );
    }

    /// Only an attached terminal without `--yes` may prompt; every other combination must stay silent,
    /// which is what keeps a piped or approved run from blocking or inventing a decision.
    #[test]
    fn only_an_attached_terminal_without_approval_prompts() {
        assert!(interactive(false, true));
        assert!(!interactive(true, true));
        assert!(!interactive(false, false));
        assert!(!interactive(true, false));
    }

    /// Rejected, unresolved and unverified entries are the only ways a run is incomplete; every omitted
    /// reason, including the home-edit ones, stays a complete success.
    #[test]
    fn only_open_entries_change_the_exit_class() {
        let omitted = [
            Disposition::Omitted {
                reason: OmissionReason::HomeModified {
                    home_changed_fields: vec!["/body".into()],
                },
            },
            Disposition::Omitted {
                reason: OmissionReason::TargetUnsupported {
                    field: "/embedding/vector".into(),
                },
            },
            Disposition::Omitted {
                reason: OmissionReason::SecretReferenceUnsupported,
            },
        ];
        let accepted = [Disposition::Accepted];
        for disposition in &omitted {
            assert_eq!(
                exit_code_for(failure_counts(
                    [disposition].into_iter(),
                    [None].into_iter()
                )),
                0,
                "{disposition:?} must not fail a run"
            );
        }
        assert_eq!(
            exit_code_for(failure_counts(accepted.iter(), [None].into_iter())),
            0
        );
        assert_eq!(
            exit_code_for(failure_counts(
                [Disposition::Rejected {
                    rule: "secret".into()
                }]
                .iter(),
                [None].into_iter()
            )),
            3
        );
        assert_eq!(
            exit_code_for(failure_counts(
                [Disposition::Unresolved {
                    reason: UnresolvedReason::TargetModified
                }]
                .iter(),
                [None].into_iter()
            )),
            3
        );
        assert_eq!(
            exit_code_for(failure_counts(
                [Disposition::Accepted].iter(),
                [Some(&Verification::Mismatch { diff: vec![] })].into_iter()
            )),
            3
        );
    }

    /// Builds one two-sided cluster of the shape the engine reports for a four-rule conflict.
    fn cluster(cluster_id: &str) -> ConflictCluster {
        serde_json::from_value(serde_json::json!({
            "cluster_id": cluster_id,
            "candidates": [
                {
                    "canonical_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "basis": "satellite:direct",
                    "origin": {"kind": "satellite", "id": "direct"},
                    "content_hash": format!("sha256:{}", "a".repeat(64)),
                    "record_hash": format!("sha256:{}", "b".repeat(64)),
                },
                {
                    "canonical_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "basis": "home:memories/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "origin": {"kind": "home", "path": "memories/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md"},
                    "content_hash": format!("sha256:{}", "c".repeat(64)),
                    "record_hash": format!("sha256:{}", "d".repeat(64)),
                }
            ]
        }))
        .unwrap()
    }

    /// A chosen side becomes a keep verdict naming that basis; leaving the cluster alone records nothing.
    #[test]
    fn decisions_follow_the_answered_choice() {
        let clusters = vec![cluster(&format!("sha256:{}", "e".repeat(64)))];
        for (answer, expected) in [
            ("1", Some("satellite:direct")),
            ("2", Some("home:memories/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
            ("3", None),
            ("", None),
        ] {
            let mut ask = |_prompt: &str| Ok(Some(answer.to_string()));
            let decisions = collect_decisions(&clusters, &mut ask).unwrap();
            match expected {
                Some(basis) => assert_eq!(
                    decisions,
                    vec![Verdict::Keep {
                        cluster_id: clusters[0].cluster_id.clone(),
                        canonical_ids: vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()],
                        bases: Some(vec![basis.into()]),
                    }],
                    "answer {answer:?}"
                ),
                None => assert!(decisions.is_empty(), "answer {answer:?}"),
            }
        }
        // End of input is a missing choice, not an empty one, so it never invents a verdict.
        let mut closed = |_prompt: &str| Ok(None);
        assert!(
            collect_decisions(&clusters, &mut closed)
                .unwrap()
                .is_empty()
        );
    }

    /// A cluster whose candidates are not one record's two sides is left undecided, never guessed at.
    #[test]
    fn unusual_clusters_are_left_undecided() {
        let mut single = cluster(&format!("sha256:{}", "f".repeat(64)));
        single.candidates.pop();
        let mut ask = |_prompt: &str| Ok(Some("1".to_string()));
        assert!(collect_decisions(&[single], &mut ask).unwrap().is_empty());
        let mut foreign = cluster(&format!("sha256:{}", "f".repeat(64)));
        foreign.candidates[1].canonical_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        assert!(collect_decisions(&[foreign], &mut ask).unwrap().is_empty());
    }

    /// Bulk answers settle every open cluster with a single question and no per-cluster prompts, and
    /// still record one verdict per cluster bound to that cluster's id, so a bulk choice cannot leak
    /// into another cluster's state (#43).
    #[test]
    fn bulk_answers_settle_every_open_cluster_at_once() {
        let first = cluster(&format!("sha256:{}", "e".repeat(64)));
        let mut second = cluster(&format!("sha256:{}", "f".repeat(64)));
        for candidate in &mut second.candidates {
            candidate.canonical_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        }
        second.candidates[1].basis = "home:memories/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
        second.candidates[1].origin = CandidateOrigin::Home {
            path: "memories/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.md".into(),
        };
        let clusters = vec![first, second];
        for (answer, prefix) in [("s", "satellite:"), ("h", "home:")] {
            let mut asks = 0;
            let mut ask = |_prompt: &str| {
                asks += 1;
                Ok(Some(answer.to_string()))
            };
            let decisions = collect_decisions(&clusters, &mut ask).unwrap();
            assert_eq!(
                asks, 1,
                "a bulk answer must not trigger per-cluster prompts"
            );
            assert_eq!(decisions.len(), 2, "one verdict per open cluster");
            let ids: std::collections::HashSet<_> = decisions
                .iter()
                .map(|verdict| match verdict {
                    Verdict::Keep { cluster_id, .. } => cluster_id.clone(),
                    _ => panic!("bulk keep produces keep verdicts"),
                })
                .collect();
            assert_eq!(ids.len(), 2, "each cluster is decided exactly once");
            for verdict in &decisions {
                let Verdict::Keep {
                    cluster_id,
                    canonical_ids,
                    bases,
                } = verdict
                else {
                    panic!("bulk keep produces keep verdicts")
                };
                let bases = bases.as_ref().expect("a side is named");
                assert_eq!(bases.len(), 1);
                assert!(bases[0].starts_with(prefix), "{bases:?}");
                assert_eq!(canonical_ids.len(), 1);
                assert!(
                    canonical_ids[0] == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        || canonical_ids[0] == "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "each verdict names its own cluster's record"
                );
                let expected = if canonical_ids[0].starts_with('a') {
                    &clusters[0]
                } else {
                    &clusters[1]
                };
                assert_eq!(cluster_id, &expected.cluster_id);
            }
        }
        let mut asks = 0;
        let mut ask = |_prompt: &str| {
            asks += 1;
            Ok(Some("n".to_string()))
        };
        assert!(collect_decisions(&clusters, &mut ask).unwrap().is_empty());
        assert_eq!(asks, 1, "keep-unresolved-all asks nothing further");
    }

    /// "p" and any unrecognized bulk answer fall back to per-cluster asking, which itself leaves
    /// unresolved whatever it cannot answer, so no path invents a verdict.
    #[test]
    fn unrecognized_bulk_answers_fall_back_to_per_cluster_asking() {
        let clusters = vec![
            cluster(&format!("sha256:{}", "e".repeat(64))),
            cluster(&format!("sha256:{}", "f".repeat(64))),
        ];
        for answer in ["p", "x"] {
            let mut asks = 0;
            let mut ask = |_prompt: &str| {
                asks += 1;
                Ok(Some(answer.to_string()))
            };
            let decisions = collect_decisions(&clusters, &mut ask).unwrap();
            assert_eq!(asks, 3, "the bulk prompt plus one prompt per cluster");
            assert!(
                decisions.is_empty(),
                "an answer neither side recognizes decides nothing"
            );
        }
        let mut closed = |_prompt: &str| Ok(None);
        assert!(
            collect_decisions(&clusters, &mut closed)
                .unwrap()
                .is_empty()
        );
    }

    /// The aggregate exit rule: the most severe class seen wins, 1 > 4 > 3 > 0 (#47).
    #[test]
    fn aggregate_sync_exit_takes_the_most_severe_class() {
        assert_eq!(aggregate_sync_exit(&[]), 0);
        assert_eq!(aggregate_sync_exit(&[0, 0]), 0);
        assert_eq!(aggregate_sync_exit(&[0, 3]), 3);
        assert_eq!(aggregate_sync_exit(&[3, 4, 0]), 4);
        assert_eq!(aggregate_sync_exit(&[4, 1]), 1);
        assert_eq!(aggregate_sync_exit(&[0, 3, 1, 4]), 1);
    }

    /// A two-candidate cluster without a home basis cannot answer keep-home; that cluster is reported
    /// unresolved instead of guessing a side, while keep-satellite still settles it.
    #[test]
    fn bulk_keep_home_skips_clusters_without_a_home_basis() {
        let mut shape = cluster(&format!("sha256:{}", "e".repeat(64)));
        shape.candidates[1].basis = "satellite:other".into();
        shape.candidates[1].origin = CandidateOrigin::Satellite {
            id: "other".into(),
            label: None,
        };
        let mut ask = |_prompt: &str| Ok(Some("h".to_string()));
        assert!(
            collect_decisions(&[shape.clone()], &mut ask)
                .unwrap()
                .is_empty()
        );
        let mut ask = |_prompt: &str| Ok(Some("s".to_string()));
        let decisions = collect_decisions(&[shape], &mut ask).unwrap();
        assert_eq!(decisions.len(), 1);
        let Verdict::Keep { bases, .. } = &decisions[0] else {
            panic!("keep-satellite produces a keep verdict")
        };
        assert_eq!(
            bases.as_deref(),
            Some(["satellite:direct".to_string()].as_slice())
        );
    }
}
