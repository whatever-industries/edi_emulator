// SPDX-License-Identifier: GPL-3.0-or-later
//! Optional local-media compatibility suites and sanitized title-matrix promotion.

use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Subcommand;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;
const LOCAL_ROOT: &str = "tests-data/local/diagnostics/compatibility";
const DEFAULT_MATRIX: &str = "data/compatibility/title-matrix.json";

#[derive(Debug, Subcommand)]
pub enum CompatibilityCommand {
    /// Execute every selected manifest case in a separate bounded process.
    Run {
        /// Local JSON manifest containing firmware and disc paths.
        manifest: PathBuf,
        /// Result directory (default: ignored compatibility diagnostics).
        #[arg(long)]
        output: Option<PathBuf>,
        /// Run only these case ids. Repeat the option to select multiple cases.
        #[arg(long = "case")]
        cases: Vec<String>,
    },
    /// Merge manually accepted passing results into the sanitized title matrix.
    Promote {
        /// suite-result.json produced by `compatibility run`.
        report: PathBuf,
        /// Confirm that the named checkpoints were manually observed.
        #[arg(long)]
        accepted: bool,
        /// Tracked matrix destination.
        #[arg(long, default_value = DEFAULT_MATRIX)]
        matrix: PathBuf,
    },
    /// Print a concise summary of a local suite result.
    Report {
        /// suite-result.json produced by `compatibility run`.
        report: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuiteManifest {
    schema_version: u32,
    id: String,
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
    cases: Vec<SuiteCase>,
}

fn default_timeout_seconds() -> u64 {
    600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SuiteCase {
    id: String,
    title: String,
    rom: PathBuf,
    disc: PathBuf,
    #[serde(default)]
    dvc_rom: Option<PathBuf>,
    model: String,
    video_standard: VideoStandard,
    instructions: u64,
    #[serde(default)]
    nvram: Option<PathBuf>,
    #[serde(default)]
    click_events: Vec<String>,
    checkpoint: String,
    #[serde(default)]
    known_issues: Vec<String>,
    #[serde(default)]
    assertions: Assertions,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum VideoStandard {
    Pal,
    Ntsc,
}

impl VideoStandard {
    fn cli_name(self) -> &'static str {
        match self {
            Self::Pal => "pal",
            Self::Ntsc => "ntsc",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Assertions {
    #[serde(default)]
    minimum_frames: Option<u64>,
    #[serde(default)]
    minimum_audio_frames: Option<u64>,
    #[serde(default)]
    minimum_unique_rasters: Option<usize>,
    /// Explicit opt-in stall heuristic. Static menus should leave this unset.
    #[serde(default)]
    maximum_consecutive_identical_rasters: Option<usize>,
    #[serde(default)]
    maximum_dvc_errors: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BootEvidence {
    schema_version: u32,
    instructions: u64,
    snapshot: cdi_core::MachineDiagnosticSnapshot,
    events: Vec<cdi_core::MachineDiagnosticEvent>,
    framebuffer_sha256: String,
    audio_sha256: String,
    audio_frames: u64,
    disc: Option<cdi_disc::DiscInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CaseOutcome {
    Passed,
    FailedAssertions,
    ProcessFailed,
    TimedOut,
}

#[derive(Debug, Serialize, Deserialize)]
struct CaseResult {
    id: String,
    title: String,
    model: String,
    video_standard: String,
    dvc_required: bool,
    dvc_attached: bool,
    checkpoint: String,
    known_issues: Vec<String>,
    outcome: CaseOutcome,
    reasons: Vec<String>,
    elapsed_millis: u128,
    exit_code: Option<i32>,
    evidence_path: PathBuf,
    screenshot_path: PathBuf,
    audio_path: PathBuf,
    disc_fingerprint: Option<String>,
    frames: Option<u64>,
    audio_frames: Option<u64>,
    unique_rasters: Option<usize>,
    longest_identical_raster_run: Option<usize>,
    dvc_errors: Option<u64>,
    #[serde(default)]
    disc_content_kind: Option<cdi_disc::DiscContentKind>,
    #[serde(default)]
    cdic_lba: Option<u32>,
    #[serde(default)]
    vcd_specification_version: Option<u16>,
    #[serde(default)]
    vcd_entry_count: Option<usize>,
    #[serde(default)]
    vcd_list_count: Option<usize>,
    #[serde(default)]
    vcd_current_entry: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SuiteResult {
    schema_version: u32,
    suite_id: String,
    revision: String,
    dirty_diff_sha256: String,
    manifest_path: PathBuf,
    cases: Vec<CaseResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TitleMatrix {
    schema_version: u32,
    entries: Vec<TitleMatrixEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TitleMatrixEntry {
    disc_fingerprint: String,
    title: String,
    model: String,
    video_standard: String,
    dvc_required: bool,
    dvc_attached: bool,
    checkpoint: String,
    result: String,
    issues: Vec<String>,
    last_tested_revision: String,
}

pub fn execute(command: CompatibilityCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        CompatibilityCommand::Run {
            manifest,
            output,
            cases,
        } => run_suite(&manifest, output.as_deref(), &cases),
        CompatibilityCommand::Promote {
            report,
            accepted,
            matrix,
        } => promote(&report, accepted, &matrix),
        CompatibilityCommand::Report { report } => print_report(&report),
    }
}

fn run_suite(
    manifest_path: &Path,
    output: Option<&Path>,
    selected_cases: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest: SuiteManifest = read_json(manifest_path)?;
    validate_manifest(&manifest)?;
    let selected = selected_cases.iter().cloned().collect::<BTreeSet<_>>();
    if !selected.is_empty() {
        for id in &selected {
            if !manifest.cases.iter().any(|case| &case.id == id) {
                return Err(format!("unknown compatibility case {id:?}").into());
            }
        }
    }

    let revision = git_text(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unborn".to_owned());
    let output = output.map_or_else(
        || {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            Path::new(LOCAL_ROOT)
                .join(&manifest.id)
                .join(format!("{}-{timestamp}", short_revision(&revision)))
        },
        Path::to_path_buf,
    );
    std::fs::create_dir_all(&output)?;

    let mut results = Vec::new();
    for case in manifest
        .cases
        .iter()
        .filter(|case| selected.is_empty() || selected.contains(&case.id))
    {
        println!("running {} — {}", case.id, case.title);
        let result = run_case(case, &output, manifest.timeout_seconds)?;
        println!("  {:?}: {}", result.outcome, result.reasons.join("; "));
        results.push(result);
    }
    if results.is_empty() {
        return Err("compatibility suite selected no cases".into());
    }

    let report = SuiteResult {
        schema_version: SCHEMA_VERSION,
        suite_id: manifest.id,
        revision,
        dirty_diff_sha256: dirty_diff_hash(),
        manifest_path: manifest_path.to_path_buf(),
        cases: results,
    };
    let report_path = output.join("suite-result.json");
    write_json(&report_path, &report)?;
    println!("{}", report_path.display());
    if report
        .cases
        .iter()
        .any(|case| case.outcome != CaseOutcome::Passed)
    {
        return Err("one or more compatibility cases failed".into());
    }
    Ok(())
}

fn run_case(
    case: &SuiteCase,
    output: &Path,
    timeout_seconds: u64,
) -> Result<CaseResult, Box<dyn std::error::Error>> {
    let case_dir = output.join(&case.id);
    std::fs::create_dir_all(&case_dir)?;
    let evidence_path = case_dir.join("evidence.json");
    let screenshot_path = case_dir.join("final.png");
    let audio_path = case_dir.join("audio.wav");
    let stdout = File::create(case_dir.join("stdout.log"))?;
    let stderr = File::create(case_dir.join("stderr.log"))?;
    let mut command = boot_command(case, &evidence_path, &screenshot_path, &audio_path)?;
    command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    let start = Instant::now();
    let mut child = command.spawn()?;
    let status = wait_with_timeout(
        &mut child,
        Duration::from_secs(timeout_seconds),
        Duration::from_millis(20),
    )?;
    let elapsed_millis = start.elapsed().as_millis();

    let (outcome, reasons, evidence) = match status {
        ProcessCompletion::TimedOut => (
            CaseOutcome::TimedOut,
            vec![format!(
                "exceeded {timeout_seconds}-second wall-clock limit"
            )],
            None,
        ),
        ProcessCompletion::Exited(status) if !status.success() => (
            CaseOutcome::ProcessFailed,
            vec![format!("boot process exited with {status}")],
            None,
        ),
        ProcessCompletion::Exited(_) => {
            let evidence: BootEvidence = read_json(&evidence_path)?;
            let assessment = assess_evidence(&evidence, &case.assertions);
            let outcome = if assessment.is_empty() {
                CaseOutcome::Passed
            } else {
                CaseOutcome::FailedAssertions
            };
            (outcome, assessment, Some(evidence))
        }
    };

    let stats = evidence.as_ref().map(evidence_stats);
    let vcd_current_entry = evidence.as_ref().and_then(|evidence| {
        let navigation = evidence.disc.as_ref()?.vcd_navigation.as_ref()?;
        vcd_entry_at_lba(navigation, evidence.snapshot.cdic.current_lba)
    });
    Ok(CaseResult {
        id: case.id.clone(),
        title: case.title.clone(),
        model: case.model.clone(),
        video_standard: case.video_standard.cli_name().to_owned(),
        dvc_required: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .is_some_and(|disc| disc.requires_dvc),
        dvc_attached: case.dvc_rom.is_some(),
        checkpoint: case.checkpoint.clone(),
        known_issues: case.known_issues.clone(),
        outcome,
        reasons: if reasons.is_empty() {
            vec!["all explicit assertions passed".to_owned()]
        } else {
            reasons
        },
        elapsed_millis,
        exit_code: match status {
            ProcessCompletion::Exited(status) => status.code(),
            ProcessCompletion::TimedOut => None,
        },
        evidence_path,
        screenshot_path,
        audio_path,
        disc_fingerprint: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .map(|disc| disc.fingerprint.sha1.clone()),
        frames: evidence
            .as_ref()
            .map(|evidence| evidence.snapshot.mcd212.frame_count),
        audio_frames: evidence.as_ref().map(|evidence| evidence.audio_frames),
        unique_rasters: stats.as_ref().map(|stats| stats.unique_rasters),
        longest_identical_raster_run: stats
            .as_ref()
            .map(|stats| stats.longest_identical_raster_run),
        dvc_errors: evidence.as_ref().map(total_dvc_errors),
        disc_content_kind: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .map(|disc| disc.content_kind),
        cdic_lba: evidence
            .as_ref()
            .map(|evidence| evidence.snapshot.cdic.current_lba),
        vcd_specification_version: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .and_then(|disc| disc.vcd_navigation.as_ref())
            .map(|navigation| navigation.specification_version),
        vcd_entry_count: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .and_then(|disc| disc.vcd_navigation.as_ref())
            .map(|navigation| navigation.entries.len()),
        vcd_list_count: evidence
            .as_ref()
            .and_then(|evidence| evidence.disc.as_ref())
            .and_then(|disc| disc.vcd_navigation.as_ref())
            .map(|navigation| navigation.lists.len()),
        vcd_current_entry,
    })
}

fn vcd_entry_at_lba(navigation: &cdi_disc::VcdNavigationInventory, lba: u32) -> Option<u16> {
    let absolute_frame = lba.saturating_add(150);
    navigation
        .entries
        .iter()
        .filter(|entry| entry.absolute_frame <= absolute_frame)
        .max_by_key(|entry| entry.absolute_frame)
        .map(|entry| entry.number)
}

fn boot_command(
    case: &SuiteCase,
    evidence: &Path,
    screenshot: &Path,
    audio: &Path,
) -> Result<Command, Box<dyn std::error::Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("boot")
        .arg(&case.rom)
        .arg("--disc")
        .arg(&case.disc)
        .arg("--model")
        .arg(&case.model)
        .arg("--video-standard")
        .arg(case.video_standard.cli_name())
        .arg("--instructions")
        .arg(case.instructions.to_string())
        .arg("--diagnostics")
        .arg(evidence)
        .arg("--screenshot")
        .arg(screenshot)
        .arg("--audio-wav")
        .arg(audio);
    if let Some(path) = &case.dvc_rom {
        command.arg("--dvc-rom").arg(path);
    }
    if let Some(path) = &case.nvram {
        command.arg("--nvram").arg(path);
    }
    for event in &case.click_events {
        command.arg("--click-event").arg(event);
    }
    Ok(command)
}

#[derive(Debug, Clone, Copy)]
enum ProcessCompletion {
    Exited(ExitStatus),
    TimedOut,
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    poll_interval: Duration,
) -> std::io::Result<ProcessCompletion> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(ProcessCompletion::Exited(status));
        }
        if start.elapsed() >= timeout {
            child.kill()?;
            child.wait()?;
            return Ok(ProcessCompletion::TimedOut);
        }
        thread::sleep(poll_interval);
    }
}

#[derive(Debug, Clone, Copy)]
struct EvidenceStats {
    unique_rasters: usize,
    longest_identical_raster_run: usize,
}

fn evidence_stats(evidence: &BootEvidence) -> EvidenceStats {
    let hashes = evidence.events.iter().filter_map(|event| match event {
        cdi_core::MachineDiagnosticEvent::Frame { raster_hash, .. } => Some(*raster_hash),
        _ => None,
    });
    let mut unique = BTreeSet::new();
    let mut previous = None;
    let mut current_run = 0usize;
    let mut longest_run = 0usize;
    for hash in hashes {
        unique.insert(hash);
        if previous == Some(hash) {
            current_run += 1;
        } else {
            current_run = 1;
            previous = Some(hash);
        }
        longest_run = longest_run.max(current_run);
    }
    EvidenceStats {
        unique_rasters: unique.len(),
        longest_identical_raster_run: longest_run,
    }
}

fn assess_evidence(evidence: &BootEvidence, assertions: &Assertions) -> Vec<String> {
    let mut failures = Vec::new();
    let stats = evidence_stats(evidence);
    if let Some(minimum) = assertions.minimum_frames {
        let actual = evidence.snapshot.mcd212.frame_count;
        if actual < minimum {
            failures.push(format!(
                "expected at least {minimum} frames, observed {actual}"
            ));
        }
    }
    if let Some(minimum) = assertions.minimum_audio_frames {
        if evidence.audio_frames < minimum {
            failures.push(format!(
                "expected at least {minimum} audio frames, observed {}",
                evidence.audio_frames
            ));
        }
    }
    if let Some(minimum) = assertions.minimum_unique_rasters {
        if stats.unique_rasters < minimum {
            failures.push(format!(
                "expected at least {minimum} unique rasters, observed {}",
                stats.unique_rasters
            ));
        }
    }
    if let Some(maximum) = assertions.maximum_consecutive_identical_rasters {
        if stats.longest_identical_raster_run > maximum {
            failures.push(format!(
                "raster was unchanged for {} consecutive frames (maximum {maximum})",
                stats.longest_identical_raster_run
            ));
        }
    }
    if let Some(maximum) = assertions.maximum_dvc_errors {
        let actual = total_dvc_errors(evidence);
        if actual > maximum {
            failures.push(format!("observed {actual} DVC errors (maximum {maximum})"));
        }
    }
    failures
}

fn total_dvc_errors(evidence: &BootEvidence) -> u64 {
    evidence.snapshot.dvc.as_ref().map_or(0, |stats| {
        stats.demux_errors + stats.video_errors + stats.audio_errors + stats.stream_errors
    })
}

fn validate_manifest(manifest: &SuiteManifest) -> Result<(), Box<dyn std::error::Error>> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported compatibility manifest schema {}",
            manifest.schema_version
        )
        .into());
    }
    validate_id(&manifest.id)?;
    if manifest.timeout_seconds == 0 {
        return Err("timeout_seconds must be greater than zero".into());
    }
    if manifest.cases.is_empty() {
        return Err("compatibility manifest must contain at least one case".into());
    }
    let mut ids = BTreeSet::new();
    for case in &manifest.cases {
        validate_id(&case.id)?;
        if !ids.insert(&case.id) {
            return Err(format!("duplicate compatibility case id {:?}", case.id).into());
        }
        if case.instructions == 0 {
            return Err(format!("case {:?} has a zero instruction limit", case.id).into());
        }
        if case.checkpoint.trim().is_empty() {
            return Err(format!("case {:?} has no checkpoint description", case.id).into());
        }
    }
    Ok(())
}

fn promote(
    report_path: &Path,
    accepted: bool,
    matrix_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !accepted {
        return Err("promotion requires --accepted manual checkpoint confirmation".into());
    }
    let report: SuiteResult = read_json(report_path)?;
    let mut matrix = if matrix_path.exists() {
        read_json(matrix_path)?
    } else {
        TitleMatrix {
            schema_version: SCHEMA_VERSION,
            entries: Vec::new(),
        }
    };
    if matrix.schema_version != SCHEMA_VERSION {
        return Err(format!("unsupported title-matrix schema {}", matrix.schema_version).into());
    }

    let mut promoted = 0usize;
    for case in report
        .cases
        .iter()
        .filter(|case| case.outcome == CaseOutcome::Passed)
    {
        let fingerprint = case.disc_fingerprint.clone().ok_or_else(|| {
            format!(
                "case {:?} has no disc fingerprint and cannot be promoted",
                case.id
            )
        })?;
        let entry = TitleMatrixEntry {
            disc_fingerprint: fingerprint,
            title: case.title.clone(),
            model: case.model.clone(),
            video_standard: case.video_standard.clone(),
            dvc_required: case.dvc_required,
            dvc_attached: case.dvc_attached,
            checkpoint: case.checkpoint.clone(),
            result: "manually-accepted".to_owned(),
            issues: case.known_issues.clone(),
            last_tested_revision: report.revision.clone(),
        };
        if let Some(existing) = matrix.entries.iter_mut().find(|existing| {
            existing.disc_fingerprint == entry.disc_fingerprint
                && existing.model == entry.model
                && existing.video_standard == entry.video_standard
                && existing.dvc_attached == entry.dvc_attached
        }) {
            *existing = entry;
        } else {
            matrix.entries.push(entry);
        }
        promoted += 1;
    }
    matrix.entries.sort_by(|left, right| {
        (&left.title, &left.disc_fingerprint, &left.video_standard).cmp(&(
            &right.title,
            &right.disc_fingerprint,
            &right.video_standard,
        ))
    });
    write_json(matrix_path, &matrix)?;
    println!("promoted {promoted} case(s) to {}", matrix_path.display());
    Ok(())
}

fn print_report(report_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report: SuiteResult = read_json(report_path)?;
    println!(
        "{} at {} (dirty diff {})",
        report.suite_id, report.revision, report.dirty_diff_sha256
    );
    for case in report.cases {
        println!(
            "- {}: {:?}; frames={}; audio={}; rasters={}; longest-static={}; dvc-errors={}",
            case.title,
            case.outcome,
            display_option(case.frames),
            display_option(case.audio_frames),
            display_option(case.unique_rasters),
            display_option(case.longest_identical_raster_run),
            display_option(case.dvc_errors),
        );
        if let Some(kind) = case.disc_content_kind {
            println!(
                "  media={kind:?}; cdic-lba={}; vcd-version={}; entries={}; lists={}; current-entry={}",
                display_option(case.cdic_lba),
                case.vcd_specification_version
                    .map(format_vcd_version)
                    .unwrap_or_else(|| "n/a".to_owned()),
                display_option(case.vcd_entry_count),
                display_option(case.vcd_list_count),
                display_option(case.vcd_current_entry),
            );
        }
        for reason in case.reasons {
            println!("  {reason}");
        }
    }
    Ok(())
}

fn format_vcd_version(version: u16) -> String {
    format!("{}.{}", version >> 8, version & 0xff)
}

fn display_option<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "n/a".to_owned(), |value| value.to_string())
}

fn validate_id(id: &str) -> Result<(), Box<dyn std::error::Error>> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("suite and case ids must use lowercase letters, numbers, and hyphens".into());
    }
    Ok(())
}

fn short_revision(revision: &str) -> &str {
    revision.get(..revision.len().min(12)).unwrap_or(revision)
}

fn git_text(arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn dirty_diff_hash() -> String {
    use sha2::{Digest, Sha256};
    let bytes = Command::new("git")
        .args(["diff", "--binary", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
        .unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(frame_hashes: &[u64], frames: u64, audio_frames: u64) -> BootEvidence {
        let mut machine = cdi_core::Machine::new(
            cdi_core::boards::model_by_id("cdi220b").unwrap(),
            vec![0; 0x80000],
        )
        .unwrap();
        let mut events = Vec::new();
        for (index, raster_hash) in frame_hashes.iter().enumerate() {
            events.push(cdi_core::MachineDiagnosticEvent::Frame {
                cycle: index as u64,
                frame: index as u64,
                geometry: machine.bus.mcd212.display_geometry(),
                plane_a_hash: 0,
                plane_b_hash: 0,
                raster_hash: *raster_hash,
            });
        }
        machine.bus.mcd212.frame_count = frames;
        BootEvidence {
            schema_version: 1,
            instructions: 100,
            snapshot: machine.diagnostic_snapshot(),
            events,
            framebuffer_sha256: "frame".into(),
            audio_sha256: "audio".into(),
            audio_frames,
            disc: None,
        }
    }

    #[test]
    fn explicit_stall_threshold_flags_a_long_static_run() {
        let evidence = evidence(&[1, 2, 2, 2, 2], 5, 0);
        let failures = assess_evidence(
            &evidence,
            &Assertions {
                maximum_consecutive_identical_rasters: Some(3),
                ..Assertions::default()
            },
        );
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("4 consecutive"));
    }

    #[test]
    fn static_menu_is_not_called_a_stall_without_an_explicit_threshold() {
        let evidence = evidence(&[7, 7, 7, 7], 4, 0);
        assert!(assess_evidence(&evidence, &Assertions::default()).is_empty());
    }

    #[test]
    fn frame_audio_and_unique_raster_assertions_are_independent() {
        let evidence = evidence(&[1, 1, 2], 20, 100);
        let failures = assess_evidence(
            &evidence,
            &Assertions {
                minimum_frames: Some(21),
                minimum_audio_frames: Some(101),
                minimum_unique_rasters: Some(3),
                ..Assertions::default()
            },
        );
        assert_eq!(failures.len(), 3);
    }

    #[test]
    fn duplicate_case_ids_are_rejected() {
        let case = SuiteCase {
            id: "case".into(),
            title: "Title".into(),
            rom: "rom".into(),
            disc: "disc".into(),
            dvc_rom: None,
            model: "cdi220b".into(),
            video_standard: VideoStandard::Pal,
            instructions: 1,
            nvram: None,
            click_events: Vec::new(),
            checkpoint: "menu".into(),
            known_issues: Vec::new(),
            assertions: Assertions::default(),
        };
        let manifest = SuiteManifest {
            schema_version: 1,
            id: "suite".into(),
            timeout_seconds: 1,
            cases: vec![case.clone(), case],
        };
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
    }

    #[test]
    fn promoted_matrix_contains_no_local_paths() {
        let matrix = TitleMatrix {
            schema_version: 1,
            entries: vec![TitleMatrixEntry {
                disc_fingerprint: "abc".into(),
                title: "Title".into(),
                model: "cdi220b".into(),
                video_standard: "pal".into(),
                dvc_required: false,
                dvc_attached: false,
                checkpoint: "menu".into(),
                result: "manually-accepted".into(),
                issues: Vec::new(),
                last_tested_revision: "revision".into(),
            }],
        };
        let json = serde_json::to_string(&matrix).unwrap();
        assert!(!json.contains("/Users/"));
        assert!(!json.contains("/Volumes/"));
        assert!(!json.contains(".cue"));
    }

    #[test]
    fn vcd_entry_lookup_uses_absolute_disc_time() {
        let navigation = cdi_disc::VcdNavigationInventory {
            specification_version: 0x0200,
            album_id: "TEST".into(),
            volume_count: 1,
            volume_number: 1,
            psd_bytes: 0,
            offset_multiplier: 8,
            maximum_list_id: 0,
            entries: vec![
                cdi_disc::VcdEntryInventory {
                    number: 1,
                    track: 2,
                    minute: 0,
                    second: 2,
                    frame: 0,
                    absolute_frame: 150,
                },
                cdi_disc::VcdEntryInventory {
                    number: 2,
                    track: 2,
                    minute: 0,
                    second: 3,
                    frame: 0,
                    absolute_frame: 225,
                },
            ],
            lists: Vec::new(),
        };

        assert_eq!(vcd_entry_at_lba(&navigation, 0), Some(1));
        assert_eq!(vcd_entry_at_lba(&navigation, 74), Some(1));
        assert_eq!(vcd_entry_at_lba(&navigation, 75), Some(2));
    }
}
