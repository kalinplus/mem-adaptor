//! Implements the local CLI boundary: plan saves proposed work; apply records approval, writes, and saves a receipt.
//! Registered adapters handle formats while the core engine checks the execution basis and read-back evidence.
//! Approval is saved before engine checks; failures after writing may leave changed targets without a saved receipt.

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use mem_adaptor_core::engine::{Engine, SCHEMA_VERSION, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::Registry;
use mem_adaptor_core::reports::PlanReport;
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
    after_help = "D1 work in progress: synthetic inputs only. Signature secret detection is local; PII checks, home configuration, and independent conformance are still pending.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Produce a migration report without changing the target.
    Plan {
        source: PathBuf,
        #[arg(long)]
        to: String,
        #[arg(long)]
        report: PathBuf,
        /// Use the preceding receipt for idempotency and deletion protection.
        #[arg(long)]
        previous_receipt: Option<PathBuf>,
        /// Explicitly select whether secret findings pass or block.
        #[arg(long, value_parser = ["pass", "block"])]
        secret_policy: Option<String>,
        /// Allow a known rule while still reporting all its findings.
        #[arg(long)]
        allow_rule: Vec<String>,
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
}

/// Presents masked ordinary errors and a nonzero exit; it does not recover or roll back target writes.
fn main() {
    if let Err(error) = run() {
        eprintln!(
            "Error: {}",
            mem_adaptor_core::gate::mask(&format!("{error:#}"))
        );
        std::process::exit(1);
    }
}

/// Validates CLI paths/options and orchestrates plan or explicitly approved apply.
/// Saves approval before engine recomputation and the receipt after engine completion; any error propagates.
/// A saved approval is not a success receipt, and a later receipt-save error does not imply an unchanged target.
fn run() -> Result<()> {
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
        Command::Plan {
            source,
            to,
            report,
            previous_receipt,
            secret_policy,
            allow_rule,
        } => {
            let source = normalize_path(&source)?;
            let (writer, target) = to
                .split_once(':')
                .context("Target must be okf:<directory> or ump:<directory>")?;
            ensure!(
                matches!(writer, "okf" | "ump"),
                "Target must be okf:<directory> or ump:<directory>"
            );
            let target = normalize_path(Path::new(target))?;
            let report_path = normalize_path(&report)?;
            ensure!(
                !target.starts_with(&source) && !source.starts_with(&target),
                "Source and target directories must not overlap"
            );
            ensure!(
                !report_path.starts_with(&target) && !report_path.starts_with(&source),
                "Report must be outside source and target directories"
            );
            let engine = engine(&[("home".into(), writer.into(), target)])?;
            let selected = secret_policy.is_some() || !allow_rule.is_empty();
            let policy = GatePolicy {
                secrets: if secret_policy.as_deref() == Some("block") {
                    GateAction::Block
                } else {
                    GateAction::Pass
                },
                high_risk_pii: GateAction::Pass,
                rule_allowlist: allow_rule,
                origin: if selected {
                    PolicyOrigin::UserChoice
                } else {
                    PolicyOrigin::Default
                },
                user_selected: selected,
            };
            let previous_receipt = previous_receipt
                .as_deref()
                .map(normalize_path)
                .transpose()?;
            let report = engine.plan_with_previous(&source, policy, previous_receipt.as_deref())?;
            write_json_new(&report_path, &report)?;
            println!("Plan: {} records; target unchanged.", report.entries.len());
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
            println!(
                "WARNING: synthetic inputs only; home configuration and real-data conformance are pending."
            );
        }
        Command::Apply { plan, receipt, yes } => {
            let plan_path = normalize_path(&plan)?;
            let report: PlanReport = serde_json::from_slice(&fs::read(&plan_path)?)
                .map_err(|_| anyhow::anyhow!("Invalid plan report JSON or fields"))?;
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
                        normalize_path(Path::new(&target.location))?,
                    ))
                })
                .collect::<Result<_>>()?;
            let engine = engine(&targets)?;
            let receipt_path = normalize_path(
                &receipt.unwrap_or_else(|| plan_path.with_extension("receipt.json")),
            )?;
            let approval_path = plan_path.with_extension("approval.json");
            for path in [&receipt_path, &approval_path] {
                ensure!(!path.exists(), "Output report already exists");
                ensure!(
                    !path.starts_with(Path::new(&report.source.location)),
                    "Report cannot be inside the source"
                );
                ensure!(
                    targets
                        .iter()
                        .all(|(_, _, target)| !path.starts_with(target)),
                    "Report cannot be inside a target"
                );
            }
            ensure!(
                receipt_path != approval_path && receipt_path != plan_path,
                "Report paths must be distinct"
            );
            if !yes {
                ensure!(
                    io::stdin().is_terminal(),
                    "Noninteractive apply requires explicit --yes"
                );
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
            write_json_new(&approval_path, &approval)?;
            let receipt = engine.apply(
                &report,
                &approval,
                plan_path.to_string_lossy().into_owned(),
                approval_path.to_string_lossy().into_owned(),
            )?;
            // Engine execution is complete; receipt persistence can still fail after target writes.
            write_json_new(&receipt_path, &receipt)?;
            println!("Receipt: {} records.", receipt.entries.len());
            print_gate_summary(
                &receipt.gate_policy,
                receipt
                    .entries
                    .iter()
                    .flat_map(|entry| entry.sensitive_findings.iter()),
            );
        }
    }
    Ok(())
}

/// Registers local Readers and already selected target Writers; core code remains adapter-independent.
fn engine(targets: &[(String, String, PathBuf)]) -> Result<Engine> {
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader)?;
    registry.register_reader(ChatgptReader)?;
    registry.register_reader(ClaudeReader)?;
    for (id, writer, path) in targets {
        match writer.as_str() {
            "okf" => registry.register_writer(id.clone(), OkfWriter::new(path.clone()))?,
            "ump" => registry.register_writer(id.clone(), UmpWriter::new(path.clone()))?,
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
