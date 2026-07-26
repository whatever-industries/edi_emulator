// SPDX-License-Identifier: GPL-3.0-or-later
//! Local incident lifecycle and context-aware experiment memory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Subcommand;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u32 = 1;
const LOCAL_ROOT: &str = "tests-data/local/diagnostics";
const TRACKED_ROOT: &str = "data/compatibility/incidents";

#[derive(Debug, Subcommand)]
pub enum DiagnoseCommand {
    /// Create an ignored local incident with a stable evidence schema.
    Init {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        title: String,
        #[arg(long)]
        symptom: String,
        #[arg(long)]
        expected: String,
        #[arg(long = "component")]
        components: Vec<String>,
        #[arg(long)]
        disc: Option<PathBuf>,
    },
    /// Run the exact bounded scenario and store machine evidence.
    Run {
        incident: PathBuf,
        #[arg(long)]
        rom: PathBuf,
        #[arg(long)]
        disc: Option<PathBuf>,
        #[arg(long)]
        dvc_rom: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        video_standard: Option<String>,
        #[arg(long, default_value_t = 100_000)]
        instructions: u64,
        #[arg(long = "click-event")]
        click_events: Vec<String>,
        /// Confirm that this run reproduced the reported symptom.
        #[arg(long)]
        symptom_reproduced: bool,
    },
    /// Locate the first differing event or final snapshot field.
    Compare { left: PathBuf, right: PathBuf },
    /// Search compact local and tracked incident/experiment history.
    History {
        query: String,
        #[arg(long)]
        include_local: bool,
        /// Compare prior experiments with a run's context.json.
        #[arg(long)]
        context: Option<PathBuf>,
    },
    /// Generate tailored manual and neighboring regression checks.
    Verify {
        incident: PathBuf,
        /// Record that the expected behavior passed manual verification.
        #[arg(long)]
        accepted: bool,
        /// Short manual verification note (used with --accepted).
        #[arg(long)]
        notes: Option<String>,
    },
    /// Create a sanitized tracked record after reproduction is confirmed.
    Promote {
        incident: PathBuf,
        #[arg(long)]
        reproduced: bool,
    },
    /// Summarize tracked compatibility incidents and unresolved work.
    Report,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub symptom: String,
    pub expected: String,
    pub status: String,
    /// Commit checked out when the incident was first recorded.
    #[serde(default)]
    pub reported_revision: Option<String>,
    /// Most recent commit on which the symptom was reproduced.
    #[serde(default)]
    pub last_reproduced_revision: Option<String>,
    /// Most recent commit on which a human verified the expected behavior.
    #[serde(default)]
    pub last_verified_revision: Option<String>,
    /// `current`, `needs-revalidation`, `historical`, or `untracked`.
    #[serde(default = "default_evidence_status")]
    pub evidence_status: String,
    /// Why earlier evidence may no longer apply to the current build.
    #[serde(default)]
    pub revalidation_reason: Option<String>,
    pub components: Vec<String>,
    pub disc_fingerprint: Option<String>,
    pub scenario: Scenario,
    pub hypotheses: Vec<Hypothesis>,
    pub experiments: Vec<Experiment>,
    pub manual_verification: Vec<ManualVerification>,
}

fn default_evidence_status() -> String {
    "untracked".to_owned()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scenario {
    pub model: Option<String>,
    pub video_standard: Option<String>,
    pub dvc: Option<String>,
    pub instruction_limit: Option<u64>,
    pub input_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hypothesis {
    pub id: String,
    pub hardware_explanation: String,
    pub supporting_evidence: Vec<String>,
    pub contradicting_evidence: Vec<String>,
    pub falsifying_test: String,
    pub citations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: String,
    pub context: ExperimentContext,
    pub hypothesis_id: String,
    pub strategy: String,
    pub symbols: Vec<String>,
    pub assumptions: Vec<String>,
    pub expected: String,
    pub actual: String,
    pub outcome: ExperimentOutcome,
    pub regressions: Vec<String>,
    pub improvements: Vec<String>,
    pub reverted: bool,
    pub evidence_quality: String,
    pub confidence: String,
    pub applicability: Applicability,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentContext {
    pub base_revision: String,
    pub dirty_diff_hash: String,
    pub component_fingerprints: BTreeMap<String, String>,
    pub dependencies: BTreeMap<String, String>,
    pub disc_fingerprint: String,
    pub model: String,
    pub video_standard: String,
    pub dvc: String,
    pub scenario: String,
    pub timing: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExperimentOutcome {
    HypothesisFalsified,
    ImplementationFailed,
    BlockedByPrerequisite,
    RegressionCausing,
    Partial,
    Inconclusive,
    Confirmed,
    Superseded,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Applicability {
    pub conditions: Vec<String>,
    pub invalidated_by: Vec<String>,
    pub repeat_when: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualVerification {
    pub result: String,
    pub revision: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalPaths {
    rom: Option<PathBuf>,
    disc: Option<PathBuf>,
    dvc_rom: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextRelation {
    Equivalent,
    ChangedPrerequisites,
    Related,
}

pub fn execute(command: DiagnoseCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        DiagnoseCommand::Init {
            id,
            title,
            symptom,
            expected,
            components,
            disc,
        } => init(id, title, symptom, expected, components, disc.as_deref()),
        DiagnoseCommand::Run {
            incident,
            rom,
            disc,
            dvc_rom,
            model,
            video_standard,
            instructions,
            click_events,
            symptom_reproduced,
        } => run(
            &incident,
            &rom,
            disc.as_deref(),
            dvc_rom.as_deref(),
            model.as_deref(),
            video_standard.as_deref(),
            instructions,
            &click_events,
            symptom_reproduced,
        ),
        DiagnoseCommand::Compare { left, right } => compare(&left, &right),
        DiagnoseCommand::History {
            query,
            include_local,
            context,
        } => history(&query, include_local, context.as_deref()),
        DiagnoseCommand::Verify {
            incident,
            accepted,
            notes,
        } => verify(&incident, accepted, notes.as_deref()),
        DiagnoseCommand::Promote {
            incident,
            reproduced,
        } => promote(&incident, reproduced),
        DiagnoseCommand::Report => report(),
    }
}

fn init(
    id: Option<String>,
    title: String,
    symptom: String,
    expected: String,
    components: Vec<String>,
    disc: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = id.unwrap_or_else(|| {
        let mut hash = Sha256::new();
        hash.update(title.as_bytes());
        hash.update(symptom.as_bytes());
        format!("incident-{:x}", hash.finalize())[..25].to_owned()
    });
    validate_id(&id)?;
    let directory = Path::new(LOCAL_ROOT).join(&id);
    if directory.exists() {
        return Err(format!("incident already exists: {}", directory.display()).into());
    }
    std::fs::create_dir_all(directory.join("runs"))?;
    let fingerprint = disc
        .map(cdi_disc::inspect_cue)
        .transpose()?
        .map(|inventory| inventory.fingerprint.sha1);
    let incident = Incident {
        schema_version: SCHEMA_VERSION,
        id,
        title,
        symptom,
        expected,
        status: "new".to_owned(),
        reported_revision: git_text(&["rev-parse", "HEAD"]),
        last_reproduced_revision: None,
        last_verified_revision: None,
        evidence_status: "reported-current".to_owned(),
        revalidation_reason: None,
        components,
        disc_fingerprint: fingerprint,
        scenario: Scenario::default(),
        hypotheses: Vec::new(),
        experiments: Vec::new(),
        manual_verification: Vec::new(),
    };
    write_json(&directory.join("incident.json"), &incident)?;
    write_json(
        &directory.join("local-paths.json"),
        &LocalPaths {
            rom: None,
            disc: disc.map(Path::to_path_buf),
            dvc_rom: None,
        },
    )?;
    println!("{}", directory.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run(
    incident_path: &Path,
    rom: &Path,
    disc: Option<&Path>,
    dvc_rom: Option<&Path>,
    model: Option<&str>,
    video_standard: Option<&str>,
    instructions: u64,
    click_events: &[String],
    symptom_reproduced: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = incident_directory(incident_path);
    let incident_file = directory.join("incident.json");
    let mut incident: Incident = read_json(&incident_file)?;
    let run_number = std::fs::read_dir(directory.join("runs"))?.count() + 1;
    let run_dir = directory.join("runs").join(format!("run-{run_number:04}"));
    std::fs::create_dir_all(&run_dir)?;
    let evidence = run_dir.join("evidence.json");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("boot")
        .arg(rom)
        .arg("--instructions")
        .arg(instructions.to_string())
        .arg("--diagnostics")
        .arg(&evidence);
    if let Some(path) = disc {
        command.arg("--disc").arg(path);
    }
    if let Some(path) = dvc_rom {
        command.arg("--dvc-rom").arg(path);
    }
    if let Some(value) = model {
        command.arg("--model").arg(value);
    }
    if let Some(value) = video_standard {
        command.arg("--video-standard").arg(value);
    }
    for event in click_events {
        command.arg("--click-event").arg(event);
    }
    let output = command.output()?;
    std::fs::write(run_dir.join("stdout.log"), &output.stdout)?;
    std::fs::write(run_dir.join("stderr.log"), &output.stderr)?;
    if !output.status.success() {
        return Err(format!("diagnostic boot failed with {}", output.status).into());
    }
    incident.status = "run-captured".to_owned();
    incident.scenario = Scenario {
        model: model.map(str::to_owned),
        video_standard: video_standard.map(str::to_owned),
        dvc: dvc_rom.map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into()
        }),
        instruction_limit: Some(instructions),
        input_events: click_events.to_vec(),
    };
    let context = current_context(&incident, dvc_rom, instructions)?;
    if symptom_reproduced {
        mark_reproduced(&mut incident, &context.base_revision);
    }
    write_json(&run_dir.join("context.json"), &context)?;
    write_json(&incident_file, &incident)?;
    write_json(
        &directory.join("local-paths.json"),
        &LocalPaths {
            rom: Some(rom.to_path_buf()),
            disc: disc.map(Path::to_path_buf),
            dvc_rom: dvc_rom.map(Path::to_path_buf),
        },
    )?;
    println!("{}", evidence.display());
    Ok(())
}

fn current_context(
    incident: &Incident,
    dvc_rom: Option<&Path>,
    instructions: u64,
) -> Result<ExperimentContext, Box<dyn std::error::Error>> {
    let base_revision = git_text(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unborn".to_owned());
    let dirty = Command::new("git")
        .args(["diff", "--binary", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
        .unwrap_or_default();
    let dirty_diff_hash = format!("{:x}", Sha256::digest(&dirty));
    let mut component_fingerprints = BTreeMap::new();
    for component in &incident.components {
        component_fingerprints.insert(component.clone(), component_hash(component)?);
    }
    let cargo_lock = std::fs::read("Cargo.lock").unwrap_or_default();
    Ok(ExperimentContext {
        base_revision,
        dirty_diff_hash,
        component_fingerprints,
        dependencies: BTreeMap::from([(
            "Cargo.lock".to_owned(),
            format!("{:x}", Sha256::digest(cargo_lock)),
        )]),
        disc_fingerprint: incident.disc_fingerprint.clone().unwrap_or_default(),
        model: incident.scenario.model.clone().unwrap_or_default(),
        video_standard: incident.scenario.video_standard.clone().unwrap_or_default(),
        dvc: dvc_rom
            .and_then(Path::file_name)
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        scenario: serde_json::to_string(&incident.scenario)?,
        timing: instructions.to_string(),
    })
}

fn component_hash(component: &str) -> Result<String, Box<dyn std::error::Error>> {
    let paths: &[&str] = match component.to_ascii_lowercase().as_str() {
        "cpu" | "timing" => &["crates/cdi-scc68070/src", "crates/cdi-core/src/machine.rs"],
        "cdic" => &["crates/cdi-core/src/cdic.rs", "crates/cdi-disc/src"],
        "mcd212" | "video" => &["crates/cdi-core/src/mcd212.rs"],
        "slave" | "input" => &["crates/cdi-core/src/slave.rs"],
        "dvc" | "mpeg" => &[
            "crates/cdi-core/src/dvc.rs",
            "crates/cdi-core/src/mpeg1_video.rs",
        ],
        "frontend" => &["crates/cdi-frontend/src"],
        _ => &["crates/cdi-core/src", "crates/cdi-disc/src"],
    };
    let mut files = Vec::new();
    for path in paths {
        collect_files(Path::new(path), &mut files)?;
    }
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        hash.update(path.to_string_lossy().as_bytes());
        hash.update([0]);
        hash.update(std::fs::read(path)?);
        hash.update([0xFF]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn collect_files(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            collect_files(&entry?.path(), output)?;
        }
    } else if path.is_file() {
        output.push(path.to_path_buf());
    }
    Ok(())
}

fn git_text(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn compare(left: &Path, right: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let left: serde_json::Value = read_json(left)?;
    let right: serde_json::Value = read_json(right)?;
    let left_events = left["events"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let right_events = right["events"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let first = first_event_divergence(left_events, right_events);
    match first {
        Some(index) => {
            println!("first event divergence: {index}");
            println!("left:  {}", left_events[index]);
            println!("right: {}", right_events[index]);
        }
        None if left_events.len() != right_events.len() => println!(
            "event streams share {} events, then lengths differ: {} vs {}",
            left_events.len().min(right_events.len()),
            left_events.len(),
            right_events.len()
        ),
        None if left["snapshot"] != right["snapshot"] => {
            println!("events match; final snapshots differ");
            print_value_differences("", &left["snapshot"], &right["snapshot"]);
        }
        None => println!("no deterministic divergence found"),
    }
    Ok(())
}

fn first_event_divergence(
    left: &[serde_json::Value],
    right: &[serde_json::Value],
) -> Option<usize> {
    left.iter()
        .zip(right)
        .position(|(left, right)| left != right)
}

fn history(
    query: &str,
    include_local: bool,
    context_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let query = query.to_ascii_lowercase();
    let proposed = context_path.map(read_json).transpose()?;
    let mut roots = vec![PathBuf::from(TRACKED_ROOT)];
    if include_local {
        roots.push(PathBuf::from(LOCAL_ROOT));
    }
    let mut found = 0;
    for path in json_files(&roots)? {
        let Ok(incident) = read_json::<Incident>(&path) else {
            continue;
        };
        let haystack = format!(
            "{} {} {} {}",
            incident.id,
            incident.title,
            incident.symptom,
            incident.components.join(" ")
        )
        .to_ascii_lowercase();
        if haystack.contains(&query) {
            found += 1;
            println!(
                "{} [{}; evidence={}] {} — {}",
                incident.id,
                incident.status,
                incident.evidence_status,
                incident.title,
                incident.symptom
            );
            println!(
                "  revisions: reported={} reproduced={} verified={}",
                incident.reported_revision.as_deref().unwrap_or("unknown"),
                incident
                    .last_reproduced_revision
                    .as_deref()
                    .unwrap_or("never"),
                incident
                    .last_verified_revision
                    .as_deref()
                    .unwrap_or("never")
            );
            if let Some(reason) = &incident.revalidation_reason {
                println!("  revalidation: {reason}");
            }
            for (index, experiment) in incident.experiments.iter().enumerate() {
                let relation = proposed
                    .as_ref()
                    .map(|current| classify_context(&experiment.context, current))
                    .or_else(|| {
                        incident
                            .experiments
                            .get(index.wrapping_sub(1))
                            .map(|previous| {
                                classify_context(&previous.context, &experiment.context)
                            })
                    });
                println!(
                    "  {} {:?} context={relation:?}: {}",
                    experiment.id, experiment.outcome, experiment.actual,
                );
            }
        }
    }
    println!("{found} matching incident(s)");
    Ok(())
}

fn verify(
    incident_path: &Path,
    accepted: bool,
    notes: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = incident_directory(incident_path);
    let incident_file = directory.join("incident.json");
    let mut incident: Incident = read_json(&incident_file)?;
    if accepted {
        let revision = git_text(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unborn".to_owned());
        mark_verified(
            &mut incident,
            &revision,
            notes.unwrap_or("Expected behavior accepted by manual testing."),
        );
        write_json(&incident_file, &incident)?;
    }
    let mut lines = vec![
        format!("# Verification: {}", incident.title),
        String::new(),
        format!("Reported symptom: {}", incident.symptom),
        format!("Expected result: {}", incident.expected),
        format!(
            "Revision evidence: reported={}, reproduced={}, verified={}, status={}",
            incident.reported_revision.as_deref().unwrap_or("unknown"),
            incident
                .last_reproduced_revision
                .as_deref()
                .unwrap_or("never"),
            incident
                .last_verified_revision
                .as_deref()
                .unwrap_or("never"),
            incident.evidence_status
        ),
        String::new(),
        "## Exact manual check".to_owned(),
        String::new(),
        "1. Start from a clean emulator launch with the recorded model, standard, DVC, and disc."
            .to_owned(),
        "2. Reproduce the recorded input sequence without extra clicks or controller input."
            .to_owned(),
        format!("3. Confirm: {}.", incident.expected),
        "4. Record result, elapsed time, and any first frame/audio moment that differs.".to_owned(),
        String::new(),
        "## Neighboring checks".to_owned(),
        String::new(),
    ];
    for check in neighboring_checks(&incident.components).into_iter().take(5) {
        lines.push(format!("- {check}"));
    }
    lines.extend([
        String::new(),
        "## Warning signs".to_owned(),
        String::new(),
        "- A different first-divergence stage, new IRQ/DMA errors, repeated audio, shifted geometry, or input behavior changes."
            .to_owned(),
        "- A fix that depends on title text, pixel content, arbitrary delay, or a forced status value."
            .to_owned(),
    ]);
    let path = directory.join("verification.md");
    std::fs::write(&path, lines.join("\n"))?;
    println!("{}", path.display());
    Ok(())
}

fn promote(incident_path: &Path, reproduced: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !reproduced {
        return Err("promotion requires --reproduced confirmation".into());
    }
    let directory = incident_directory(incident_path);
    let mut incident: Incident = read_json(&directory.join("incident.json"))?;
    let revision = incident
        .last_reproduced_revision
        .clone()
        .or_else(|| git_text(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unborn".to_owned());
    mark_reproduced(&mut incident, &revision);
    validate_id(&incident.id)?;
    std::fs::create_dir_all(TRACKED_ROOT)?;
    let destination = Path::new(TRACKED_ROOT).join(format!("{}.json", incident.id));
    write_json(&destination, &incident)?;
    println!("{}", destination.display());
    Ok(())
}

fn report() -> Result<(), Box<dyn std::error::Error>> {
    let paths = json_files(&[PathBuf::from(TRACKED_ROOT)])?;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut incidents = Vec::new();
    for path in paths {
        let Ok(incident) = read_json::<Incident>(&path) else {
            continue;
        };
        *counts.entry(incident.status.clone()).or_default() += 1;
        incidents.push(incident);
    }
    println!("{} tracked incident(s)", incidents.len());
    for (status, count) in counts {
        println!("  {status}: {count}");
    }
    for incident in incidents
        .iter()
        .filter(|incident| incident.status != "resolved")
    {
        println!(
            "- {}: {} [{}; evidence={}; last reproduced={}]",
            incident.title,
            incident.symptom,
            incident.status,
            incident.evidence_status,
            incident
                .last_reproduced_revision
                .as_deref()
                .unwrap_or("never")
        );
    }
    Ok(())
}

fn classify_context(prior: &ExperimentContext, current: &ExperimentContext) -> ContextRelation {
    if prior == current {
        ContextRelation::Equivalent
    } else if prior.dependencies != current.dependencies
        || prior.component_fingerprints != current.component_fingerprints
    {
        ContextRelation::ChangedPrerequisites
    } else {
        ContextRelation::Related
    }
}

fn mark_reproduced(incident: &mut Incident, revision: &str) {
    incident.status = "reproduced".to_owned();
    incident.last_reproduced_revision = Some(revision.to_owned());
    incident.evidence_status = "current".to_owned();
    incident.revalidation_reason = None;
}

fn mark_verified(incident: &mut Incident, revision: &str, notes: &str) {
    incident.status = "resolved".to_owned();
    incident.last_verified_revision = Some(revision.to_owned());
    incident.evidence_status = "current".to_owned();
    incident.revalidation_reason = None;
    incident.manual_verification.push(ManualVerification {
        result: "accepted".to_owned(),
        revision: revision.to_owned(),
        notes: notes.to_owned(),
    });
}

fn neighboring_checks(components: &[String]) -> Vec<&'static str> {
    let mut checks = Vec::new();
    for component in components {
        match component.to_ascii_lowercase().as_str() {
            "cpu" | "timing" => checks.extend([
                "Boot a known-good shell and verify stable frame rate.",
                "Check XA/CDDA audio cadence and mouse/controller polling.",
            ]),
            "cdic" => checks.extend([
                "Open a filesystem-heavy title and a CD-i Ready image.",
                "Check XA, CDDA, RTF delivery, VMPEG, and VCD paths.",
            ]),
            "mcd212" | "video" => checks.extend([
                "Check PAL and NTSC base graphics plus interlace and cursor animation.",
                "Verify screenshot bounds and pointer endpoints match the displayed aperture.",
            ]),
            "slave" | "input" => checks.extend([
                "Verify two-button input, pointer capture/release, and disc-launch reset.",
                "Check PAL/NTSC status reporting after reset.",
            ]),
            "dvc" | "mpeg" => checks.extend([
                "Check video/audio synchronization, pause/continue, and a base-to-FMV transition.",
                "Check VCD and repeated stream transitions for decoder errors.",
            ]),
            "frontend" => checks.extend([
                "Check window resize/aspect, screenshots, audio stop on Library, and all input modes.",
            ]),
            _ => {}
        }
    }
    checks.sort_unstable();
    checks.dedup();
    checks
}

fn print_value_differences(prefix: &str, left: &serde_json::Value, right: &serde_json::Value) {
    match (left, right) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            for key in left
                .keys()
                .chain(right.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                let next = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                print_value_differences(
                    &next,
                    left.get(key).unwrap_or(&serde_json::Value::Null),
                    right.get(key).unwrap_or(&serde_json::Value::Null),
                );
            }
        }
        _ if left != right => println!("{prefix}: {left} != {right}"),
        _ => {}
    }
}

fn incident_directory(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    }
}

fn validate_id(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("incident id must use lowercase letters, numbers, and hyphens".into());
    }
    Ok(())
}

fn json_files(roots: &[PathBuf]) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut pending = roots.to_vec();
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        if !path.exists() {
            continue;
        }
        if path.is_dir() {
            for entry in std::fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path.file_name().is_some_and(|name| name == "incident.json")
            || path
                .parent()
                .is_some_and(|parent| parent.ends_with("incidents"))
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ExperimentContext {
        ExperimentContext {
            base_revision: "abc".into(),
            dirty_diff_hash: "def".into(),
            component_fingerprints: BTreeMap::from([("cdic".into(), "1".into())]),
            dependencies: BTreeMap::from([("timing".into(), "1".into())]),
            disc_fingerprint: "disc".into(),
            model: "cdi220".into(),
            video_standard: "pal".into(),
            dvc: "none".into(),
            scenario: "boot".into(),
            timing: "100000".into(),
        }
    }

    #[test]
    fn equivalent_context_is_identified_without_blocking_repetition() {
        assert_eq!(
            classify_context(&context(), &context()),
            ContextRelation::Equivalent
        );
    }

    #[test]
    fn changed_dependency_reopens_a_previous_result() {
        let previous = context();
        let mut current = context();
        current.dependencies.insert("timing".into(), "2".into());
        assert_eq!(
            classify_context(&previous, &current),
            ContextRelation::ChangedPrerequisites
        );
    }

    #[test]
    fn outcome_taxonomy_does_not_equate_failed_code_with_false_hypothesis() {
        assert_ne!(
            ExperimentOutcome::ImplementationFailed,
            ExperimentOutcome::HypothesisFalsified
        );
    }

    #[test]
    fn compare_locates_the_first_damaged_provenance_stage() {
        let baseline = vec![
            serde_json::json!({"kind":"disc-position","lba":10}),
            serde_json::json!({"kind":"frame","plane_a_hash":1,"raster_hash":2}),
            serde_json::json!({"kind":"frame","plane_a_hash":3,"raster_hash":4}),
        ];
        let damaged = vec![
            baseline[0].clone(),
            serde_json::json!({"kind":"frame","plane_a_hash":99,"raster_hash":2}),
            baseline[2].clone(),
        ];
        assert_eq!(first_event_divergence(&baseline, &damaged), Some(1));
    }

    #[test]
    fn legacy_incidents_load_with_explicitly_untracked_revision_evidence() {
        let incident: Incident = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "id": "legacy",
            "title": "title",
            "symptom": "symptom",
            "expected": "expected",
            "status": "new",
            "components": [],
            "disc_fingerprint": null,
            "scenario": {
                "model": null,
                "video_standard": null,
                "dvc": null,
                "instruction_limit": null,
                "input_events": []
            },
            "hypotheses": [],
            "experiments": [],
            "manual_verification": []
        }))
        .unwrap();

        assert_eq!(incident.reported_revision, None);
        assert_eq!(incident.last_reproduced_revision, None);
        assert_eq!(incident.last_verified_revision, None);
        assert_eq!(incident.evidence_status, "untracked");
    }

    #[test]
    fn reproduction_and_manual_acceptance_record_distinct_revisions() {
        let mut incident = Incident {
            schema_version: 1,
            id: "revision-test".into(),
            title: "title".into(),
            symptom: "symptom".into(),
            expected: "expected".into(),
            status: "new".into(),
            reported_revision: Some("report".into()),
            last_reproduced_revision: None,
            last_verified_revision: None,
            evidence_status: "reported-current".into(),
            revalidation_reason: Some("old prerequisite".into()),
            components: Vec::new(),
            disc_fingerprint: None,
            scenario: Scenario::default(),
            hypotheses: Vec::new(),
            experiments: Vec::new(),
            manual_verification: Vec::new(),
        };

        mark_reproduced(&mut incident, "reproduce");
        assert_eq!(
            incident.last_reproduced_revision.as_deref(),
            Some("reproduce")
        );
        assert_eq!(incident.last_verified_revision, None);
        assert_eq!(incident.status, "reproduced");

        mark_verified(&mut incident, "verify", "manual pass");
        assert_eq!(incident.last_verified_revision.as_deref(), Some("verify"));
        assert_eq!(incident.status, "resolved");
        assert_eq!(incident.manual_verification.len(), 1);
        assert_eq!(incident.manual_verification[0].notes, "manual pass");
    }

    #[test]
    fn tracked_incident_schema_has_no_host_path_field() {
        let incident = Incident {
            schema_version: 1,
            id: "privacy-test".into(),
            title: "title".into(),
            symptom: "symptom".into(),
            expected: "expected".into(),
            status: "reproduced".into(),
            reported_revision: Some("abc".into()),
            last_reproduced_revision: Some("abc".into()),
            last_verified_revision: None,
            evidence_status: "current".into(),
            revalidation_reason: None,
            components: vec!["cdic".into()],
            disc_fingerprint: Some("hash".into()),
            scenario: Scenario::default(),
            hypotheses: Vec::new(),
            experiments: Vec::new(),
            manual_verification: Vec::new(),
        };
        let serialized = serde_json::to_string(&incident).unwrap();
        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains("/Volumes/"));
        assert!(!serialized.contains("local-paths"));
    }
}
