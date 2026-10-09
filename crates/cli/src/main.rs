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
use mem_adaptor_core::engine::{Engine, SCHEMA_VERSION, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::Registry;
use mem_adaptor_core::reports::{PlanReport, SatelliteSpec};
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

/// Validates CLI paths/options and orchestrates init, plan, or explicitly approved apply.
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
        } => {
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
            let mut refuse = crate::home::refuse_relocation
                as fn(&[RelocationCandidate]) -> Result<Option<String>>;
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
                        let path =
                            crate::home::plan_path(home, satellite.id.as_str(), &report.run_id);
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
        }
        Command::Apply { plan, receipt, yes } => {
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
            ensure!(
                report.targets.iter().all(|target| engine
                    .registry
                    .writer(&target.id)
                    .is_ok_and(|writer| writer.location().to_string_lossy() == target.location)),
                "[S7] Approved target path changed before target writes and before approval was saved; inspect target paths and make a new plan"
            );
            let spec = report.source.satellite.clone();
            let mut home = report
                .targets
                .iter()
                .find_map(|target| crate::home::detect(&target.writer, Path::new(&target.location)))
                .map(|directory| crate::home::load(&directory))
                .transpose()?;
            ensure!(
                home.is_none() || spec.is_some(),
                "[S7] This plan carries no satellite identity but its target is now a home; home mode requires a satellite. Plan again against that home"
            );
            ensure!(
                home.is_some()
                    || spec.is_none()
                    || !report.targets.iter().any(|target| target.writer == "okf"),
                "[S7] This plan carries a satellite identity but its okf target has no .mem-adaptor/config.toml; the home this plan was made against is missing. Restore the home configuration or plan again; applying it as a direct migration would strand this round's receipt outside the satellite's chain"
            );
            if let (Some(home), Some(spec)) = (&home, &spec) {
                crate::home::validate_plan_satellite(home, &engine.registry, spec, Path::new(&report.source.location))
                    .context(
                        "[S7] Plan satellite validation failed before target writes and before approval was saved; this execution has not written targets. Inspect the home registry and the plan's source before proceeding",
                    )?;
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
                receipt_path.as_ref() != Some(&approval_path)
                    && receipt_path.as_ref() != Some(&plan_path),
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
            write_json_new(&approval_path, &approval).context(
                "[S7] Approval save failed before target writes; targets unchanged, approval may be incomplete. Inspect the report path and create a new plan before proceeding; do not overwrite existing artifacts",
            )?;
            let receipt = engine.apply(
                &report,
                &approval,
                plan_path.to_string_lossy().into_owned(),
                approval_path.to_string_lossy().into_owned(),
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
