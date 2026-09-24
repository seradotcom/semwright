//! User-facing parsing and rendering only. Every Semwright operation goes over broker IPC.
use clap::{Args, Parser, Subcommand};
use semwright_types::*;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(
    name = "semwright",
    version,
    about = "Typed, policy-scoped Linux automation",
    long_about = "Inspect and control a Linux desktop through one local capability broker. Start semwrightd first. Use commands search/describe to discover application adapters. JSON results go to stdout; diagnostics go to stderr. No shell/eval/confirmation approval command exists."
)]
pub struct Cli {
    #[arg(long, global = true, env = "SEMWRIGHT_SOCKET")]
    pub socket: Option<PathBuf>,
    #[arg(long, global = true, env = "SEMWRIGHT_SESSION_FILE")]
    pub session_file: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        env = "SEMWRIGHT_MCP_UPSTREAMS",
        help = "Owner-only MCP upstream registry; definitions never grant policy authority"
    )]
    pub mcp_upstreams: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Emit one machine-readable JSON result; no ANSI"
    )]
    pub json: bool,
    #[arg(
        long,
        global = true,
        help = "Validate permissions and return a plan without side effects"
    )]
    pub dry_run: bool,
    #[arg(
        long,
        global = true,
        help = "Explicit backend preference; never bypasses policy"
    )]
    pub backend: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand, Debug)]
pub enum Command {
    Doctor,
    #[command(visible_alias = "capability")]
    Capabilities {
        #[command(subcommand)]
        command: Capability,
    },
    Commands {
        #[command(subcommand)]
        command: Discovery,
    },
    /// Universal gateway for every registered command, including application adapters.
    Execute {
        command: String,
        #[command(flatten)]
        args: JsonArgs,
    },
    App {
        #[command(subcommand)]
        command: App,
    },
    Window {
        #[command(subcommand)]
        command: Window,
    },
    Ui {
        #[command(subcommand)]
        command: Ui,
    },
    Input {
        #[command(subcommand)]
        command: Input,
    },
    Pointer {
        #[command(subcommand)]
        command: Pointer,
    },
    Portal {
        #[command(subcommand)]
        command: Portal,
    },
    Screen {
        #[command(subcommand)]
        command: Screen,
    },
    Clipboard {
        #[command(subcommand)]
        command: Clipboard,
    },
    /// Owner-facing configuration for federated MCP servers. Not an agent capability.
    Mcp {
        #[command(subcommand)]
        command: Mcp,
    },
    Recipe {
        #[command(subcommand)]
        command: Recipe,
    },
    /// Record, compile, verify and promote reusable operational workflows.
    Workflow {
        #[command(subcommand)]
        command: Workflow,
    },
    Plugin {
        #[command(subcommand)]
        command: Plugin,
    },
    /// Owner/developer tooling for persistent application drivers.
    Driver {
        #[command(subcommand)]
        command: DriverCommand,
    },
    Audit {
        #[command(subcommand)]
        command: Audit,
    },
    /// Stream redacted command events, not application content.
    Watch {
        #[arg(long, default_value_t = 0)]
        after: u64,
    },
    /// Print XDG paths without contacting the broker.
    Config {
        #[command(subcommand)]
        command: Config,
    },
    Completions {
        shell: clap_complete::Shell,
    },
    Man,
}
#[derive(Subcommand, Debug)]
pub enum ListOnly {
    List,
}
#[derive(Subcommand, Debug)]
pub enum Config {
    Paths,
}
#[derive(Subcommand, Debug)]
pub enum Capability {
    List,
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        risk: Option<String>,
        #[arg(long)]
        available: Option<bool>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        object_type: Vec<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long)]
        revision: Option<u64>,
    },
    Describe {
        name: String,
    },
    Execute {
        command: String,
        #[command(flatten)]
        args: JsonArgs,
    },
}
#[derive(Subcommand, Debug)]
pub enum Discovery {
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value_t = 25)]
        limit: usize,
    },
    Describe {
        name: String,
    },
}
#[derive(Args, Debug, Default)]
pub struct JsonArgs {
    #[arg(
        long,
        conflicts_with = "args_file",
        help = "JSON object; prefer --args-file - for sensitive values"
    )]
    pub args_json: Option<String>,
    #[arg(long, help = "Read a JSON object from a file, or - for stdin")]
    pub args_file: Option<PathBuf>,
}
#[derive(Args, Debug)]
pub struct Text {
    #[arg(
        long,
        conflicts_with = "text_stdin",
        required_unless_present = "text_stdin"
    )]
    pub text: Option<String>,
    #[arg(long, help = "Read exact UTF-8 text from stdin; no trimming")]
    pub text_stdin: bool,
}
#[derive(Subcommand, Debug)]
pub enum App {
    List,
    Launch { application: String },
    Close { reference: String },
}
#[derive(Subcommand, Debug)]
pub enum Window {
    List,
    Focus {
        reference: String,
    },
    Close {
        reference: String,
    },
    Move {
        reference: String,
        x: i32,
        y: i32,
    },
    Resize {
        reference: String,
        width: u32,
        height: u32,
    },
}
#[derive(Subcommand, Debug)]
pub enum Ui {
    Snapshot {
        #[arg(long)]
        app: Option<String>,
        #[arg(long, default_value_t = 200)]
        max_nodes: usize,
        #[arg(long, default_value_t = 5)]
        max_depth: usize,
        #[arg(long)]
        actionable: bool,
    },
    Find {
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long, conflicts_with = "name_regex")]
        name: Option<String>,
        #[arg(long)]
        name_regex: Option<String>,
        #[arg(long)]
        ancestor: Option<String>,
        #[arg(long)]
        action: Option<String>,
        #[arg(long)]
        state: Vec<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 500)]
        max_nodes: usize,
    },
    Invoke {
        reference: String,
        #[arg(long)]
        action: Option<String>,
    },
    SetText {
        reference: String,
        #[command(flatten)]
        text: Text,
    },
    ReadText {
        reference: String,
        #[arg(long, default_value_t = 4096)]
        max_chars: usize,
    },
    SetValue {
        reference: String,
        value: f64,
    },
    GetValue {
        reference: String,
    },
    Toggle {
        reference: String,
    },
    Expand {
        reference: String,
    },
    Select {
        reference: String,
        index: i32,
    },
}
#[derive(Subcommand, Debug)]
pub enum Input {
    Key {
        reference: String,
        keysym: u32,
    },
    Type {
        reference: String,
        #[command(flatten)]
        text: Text,
    },
}
#[derive(Subcommand, Debug)]
pub enum Pointer {
    Move {
        reference: String,
        dx: f64,
        dy: f64,
    },
    Click {
        reference: String,
        #[arg(long,default_value="left",value_parser=["left","middle","right"])]
        button: String,
    },
    Scroll {
        reference: String,
        dx: f64,
        dy: f64,
    },
}
#[derive(Subcommand, Debug)]
pub enum Portal {
    Start {
        #[arg(
            long,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true"
        )]
        keyboard: Option<bool>,
        #[arg(
            long,
            num_args = 0..=1,
            require_equals = true,
            default_missing_value = "true"
        )]
        pointer: Option<bool>,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=2))]
        persist_mode: u32,
        #[arg(long)]
        clipboard: bool,
    },
    Stop,
    Status,
    RestoreClear,
}
#[derive(Subcommand, Debug)]
pub enum Screen {
    Capture,
    StreamInfo,
    StreamStart {
        #[arg(long, default_value = "any")]
        source: String,
        #[arg(long)]
        multiple: bool,
        #[arg(long, default_value = "embedded")]
        cursor: String,
    },
    StreamCapture {
        #[arg(long, default_value_t = 0)]
        stream: usize,
        #[arg(long, default_value_t = 5_000)]
        timeout_ms: u64,
    },
    StreamStop,
}
#[derive(Subcommand, Debug)]
pub enum Clipboard {
    Read,
    Write {
        #[command(flatten)]
        text: Text,
    },
}
#[derive(Subcommand, Debug)]
pub enum Mcp {
    Upstream {
        #[command(subcommand)]
        command: McpUpstream,
    },
}
#[derive(Subcommand, Debug)]
pub enum McpUpstream {
    List,
    Inspect {
        slug: String,
    },
    Add {
        slug: String,
        program: PathBuf,
        #[arg(long = "arg", value_name = "ARG")]
        args: Vec<String>,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        expected_name: Option<String>,
        #[arg(long)]
        expected_version: Option<String>,
        #[arg(long, default_value_t = 30_000)]
        request_timeout_ms: u64,
        #[arg(long)]
        disabled: bool,
        #[arg(long)]
        replace: bool,
    },
    Enable {
        slug: String,
    },
    Disable {
        slug: String,
    },
    Remove {
        slug: String,
    },
    /// Launch the pinned executable, negotiate MCP and list its tools, then shut it down.
    Doctor {
        slug: String,
    },
}
#[derive(Subcommand, Debug)]
pub enum Recipe {
    List,
    Validate {
        file: PathBuf,
    },
    Run {
        file: PathBuf,
        #[command(flatten)]
        inputs: JsonArgs,
    },
    Test {
        file: PathBuf,
        #[command(flatten)]
        inputs: JsonArgs,
    },
    Scaffold {
        name: String,
        output: PathBuf,
    },
}
#[derive(Subcommand, Debug)]
pub enum Workflow {
    Record {
        #[command(subcommand)]
        command: WorkflowRecord,
    },
    Traces,
    Show {
        trace_id: String,
    },
    DeleteTrace {
        trace_id: String,
    },
    Compile {
        name: String,
        #[arg(long = "trace", required = true)]
        trace_ids: Vec<String>,
        #[arg(long, default_value = "")]
        description: String,
        /// Parameter hint NAME=STEP:/json/pointer. Repeat as needed.
        #[arg(long = "parameter")]
        parameters: Vec<String>,
    },
    Candidate {
        candidate_id: String,
    },
    DeleteCandidate {
        candidate_id: String,
    },
    Verify {
        candidate_id: String,
    },
    Replay {
        candidate_id: String,
        #[command(flatten)]
        inputs: JsonArgs,
    },
    Promote {
        candidate_id: String,
        slug: String,
    },
    List,
    Demote {
        slug: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum WorkflowRecord {
    Start {
        name: String,
        #[arg(long, default_value = "")]
        intent: String,
        #[arg(long)]
        capture_values: bool,
    },
    Stop {
        #[arg(long)]
        failed: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum Plugin {
    List,
    Describe {
        name: String,
    },
    Doctor {
        name: String,
    },
    Install {
        manifest: PathBuf,
    },
    Remove {
        name: String,
    },
    Scaffold {
        output: PathBuf,
        #[arg(long)]
        sdk_path: PathBuf,
    },
}
#[derive(Subcommand, Debug)]
pub enum DriverCommand {
    Validate {
        manifest: PathBuf,
    },
    Inspect {
        manifest: PathBuf,
    },
    /// Launch the pinned driver inside the production sandbox and verify protocol/health/shutdown.
    Conformance {
        manifest: PathBuf,
    },
    Scaffold {
        name: String,
        output: PathBuf,
        #[arg(long)]
        sdk_path: PathBuf,
    },
    /// Build or inspect the non-executing Semwright driver package format.
    Package {
        #[command(subcommand)]
        command: DriverPackageCommand,
    },
    /// Validate or search a static/local driver index.
    Index {
        #[command(subcommand)]
        command: DriverIndexCommand,
    },
    /// Install the selected compatible package without executing it or changing policy grants.
    Install {
        index: PathBuf,
        id: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        application_version: Option<String>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Resolve and install a newer compatible version; old version directories remain for rollback.
    Update {
        index: PathBuf,
        id: String,
        #[arg(long)]
        application_version: Option<String>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Remove one receipt-bound installed version. Policy is never edited automatically.
    Remove {
        id: String,
        version: String,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum DriverPackageCommand {
    Create {
        manifest: PathBuf,
        output: PathBuf,
        /// SemVer requirement for compatible Semwright versions. Defaults to this build exactly.
        #[arg(long)]
        semwright: Option<String>,
    },
    Inspect {
        package: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum DriverIndexCommand {
    Validate {
        index: PathBuf,
    },
    Search {
        index: PathBuf,
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        application_version: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
pub enum Audit {
    Tail {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

fn bounded(mut reader: impl Read, max: usize) -> Result<String> {
    let mut text = String::new();
    reader
        .by_ref()
        .take((max + 1) as u64)
        .read_to_string(&mut text)?;
    if text.len() > max {
        return Err(Error::invalid("Input exceeds its size budget"));
    }
    Ok(text)
}
pub fn read_file(path: &Path, max: usize) -> Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    if path == Path::new("-") {
        return bounded(std::io::stdin().lock(), max);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::invalid("Input must be a regular file or stdin"));
    }
    bounded(file, max)
}
impl JsonArgs {
    pub fn value(&self) -> Result<Value> {
        let text = match (&self.args_json, &self.args_file) {
            (Some(s), _) => s.clone(),
            (_, Some(p)) => read_file(p, 262144)?,
            _ => "{}".into(),
        };
        if text.len() > 262144 {
            return Err(Error::invalid("JSON input too large"));
        }
        let v: Value = serde_json::from_str(&text)?;
        if !v.is_object() {
            return Err(Error::invalid("Arguments must be a JSON object"));
        }
        Ok(v)
    }
}
impl Text {
    fn value(&self) -> Result<String> {
        if self.text_stdin {
            bounded(std::io::stdin().lock(), 65536)
        } else {
            let s = self
                .text
                .clone()
                .ok_or_else(|| Error::invalid("A text source is required"))?;
            if s.len() > 65536 {
                return Err(Error::invalid("Text exceeds budget"));
            }
            Ok(s)
        }
    }
}
fn read_manifest(path: &Path) -> Result<Value> {
    let text = read_file(path, 262144)?;
    let manifest: semwright_plugin_sdk::Manifest = if text.trim_start().starts_with('{') {
        serde_json::from_str(&text)?
    } else {
        toml::from_str(&text).map_err(|_| Error::invalid("Invalid plugin manifest TOML"))?
    };
    manifest.validate()?;
    serde_json::to_value(manifest).map_err(Into::into)
}
fn put(v: &mut Value, k: &str, s: &Option<String>) {
    if let Some(s) = s {
        v[k] = json!(s);
    }
}
fn workflow_parameter(value: &str) -> Result<Value> {
    let (name, location) = value
        .split_once('=')
        .ok_or_else(|| Error::invalid("Workflow parameter must be NAME=STEP:/json/pointer"))?;
    let (step, pointer) = location
        .split_once(':')
        .ok_or_else(|| Error::invalid("Workflow parameter must include STEP:/json/pointer"))?;
    let step: usize = step
        .parse()
        .map_err(|_| Error::invalid("Workflow parameter step must be an integer"))?;
    if name.is_empty() || step > 63 || (!pointer.is_empty() && !pointer.starts_with('/')) {
        return Err(Error::invalid("Workflow parameter hint is invalid"));
    }
    Ok(json!({"name":name,"step":step,"pointer":pointer,"secret":false}))
}
pub fn request(cli: &Cli) -> Result<Option<ExecuteRequest>> {
    let (cmd, args) = match &cli.command {
        Command::Doctor => ("doctor".into(), json!({})),
        Command::Capabilities { command } => match command {
            Capability::List => ("capabilities.list".into(), json!({})),
            Capability::Search {
                query,
                provider,
                app,
                category,
                risk,
                available,
                tag,
                object_type,
                limit,
                offset,
                revision,
            } => (
                "capabilities.search".into(),
                json!({"query":query,"provider":provider,"app":app,"category":category,"risk":risk,"available":available,"tags":tag,"object_types":object_type,"limit":limit,"offset":offset,"revision":revision}),
            ),
            Capability::Describe { name } => ("capabilities.describe".into(), json!({"name":name})),
            Capability::Execute { command, args } => (command.clone(), args.value()?),
        },
        Command::Commands {
            command: Discovery::Search { query, limit },
        } => (
            "commands.search".into(),
            json!({"query":query,"limit":limit}),
        ),
        Command::Commands {
            command: Discovery::Describe { name },
        } => ("commands.describe".into(), json!({"name":name})),
        Command::Execute { command, args } => (command.clone(), args.value()?),
        Command::App { command } => match command {
            App::List => ("app.list".into(), json!({})),
            App::Launch { application } => {
                ("app.launch".into(), json!({"application":application}))
            }
            App::Close { reference } => ("app.close".into(), json!({"ref":reference})),
        },
        Command::Window { command } => match command {
            Window::List => ("window.list".into(), json!({})),
            Window::Focus { reference } => ("window.focus".into(), json!({"ref":reference})),
            Window::Close { reference } => ("window.close".into(), json!({"ref":reference})),
            Window::Move { reference, x, y } => {
                ("window.move".into(), json!({"ref":reference,"x":x,"y":y}))
            }
            Window::Resize {
                reference,
                width,
                height,
            } => (
                "window.resize".into(),
                json!({"ref":reference,"width":width,"height":height}),
            ),
        },
        Command::Ui { command } => match command {
            Ui::Snapshot {
                app,
                max_nodes,
                max_depth,
                actionable,
            } => {
                let mut a =
                    json!({"max_nodes":max_nodes,"max_depth":max_depth,"actionable":actionable});
                put(&mut a, "app", app);
                ("ui.snapshot".into(), a)
            }
            Ui::Find {
                app,
                role,
                name,
                name_regex,
                ancestor,
                action,
                state,
                query,
                max_nodes,
            } => {
                let mut s = json!({"states":state});
                for (k, v) in [
                    ("app", app),
                    ("role", role),
                    ("ancestor", ancestor),
                    ("action", action),
                    ("query", query),
                ] {
                    put(&mut s, k, v);
                }
                if let Some(name) = name {
                    s["name"] = json!({"op":"exact","value":name});
                }
                if let Some(name) = name_regex {
                    s["name"] = json!({"op":"regex","value":name});
                }
                (
                    "ui.find".into(),
                    json!({"selector":s,"max_nodes":max_nodes}),
                )
            }
            Ui::Invoke { reference, action } => {
                let mut a = json!({"ref":reference});
                put(&mut a, "action", action);
                ("ui.invoke".into(), a)
            }
            Ui::SetText { reference, text } => (
                "ui.set_text".into(),
                json!({"ref":reference,"text":text.value()?}),
            ),
            Ui::ReadText {
                reference,
                max_chars,
            } => (
                "ui.read_text".into(),
                json!({"ref":reference,"max_chars":max_chars}),
            ),
            Ui::SetValue { reference, value } => (
                "ui.set_value".into(),
                json!({"ref":reference,"value":value}),
            ),
            Ui::GetValue { reference } => ("ui.get_value".into(), json!({"ref":reference})),
            Ui::Toggle { reference } => ("ui.toggle".into(), json!({"ref":reference})),
            Ui::Expand { reference } => ("ui.expand".into(), json!({"ref":reference})),
            Ui::Select { reference, index } => {
                ("ui.select".into(), json!({"ref":reference,"index":index}))
            }
        },
        Command::Input { command } => match command {
            Input::Key { reference, keysym } => {
                ("input.key".into(), json!({"ref":reference,"keysym":keysym}))
            }
            Input::Type { reference, text } => (
                "input.type".into(),
                json!({"ref":reference,"text":text.value()?}),
            ),
        },
        Command::Pointer { command } => match command {
            Pointer::Move { reference, dx, dy } => (
                "pointer.move".into(),
                json!({"ref":reference,"dx":dx,"dy":dy}),
            ),
            Pointer::Scroll { reference, dx, dy } => (
                "pointer.scroll".into(),
                json!({"ref":reference,"dx":dx,"dy":dy}),
            ),
            Pointer::Click { reference, button } => (
                "pointer.click".into(),
                json!({"ref":reference,"button":button}),
            ),
        },
        Command::Portal { command } => match command {
            Portal::Start {
                keyboard,
                pointer,
                persist_mode,
                clipboard,
            } => {
                let mut args = json!({"persist_mode":persist_mode,"clipboard":clipboard});
                if let Some(keyboard) = keyboard {
                    args["keyboard"] = json!(keyboard);
                }
                if let Some(pointer) = pointer {
                    args["pointer"] = json!(pointer);
                }
                ("portal.start".into(), args)
            }
            Portal::Stop => ("portal.stop".into(), json!({})),
            Portal::Status => ("portal.status".into(), json!({})),
            Portal::RestoreClear => ("portal.restore.clear".into(), json!({})),
        },
        Command::Screen { command } => match command {
            Screen::Capture => ("screen.capture".into(), json!({})),
            Screen::StreamInfo => ("screen.stream_info".into(), json!({})),
            Screen::StreamStart {
                source,
                multiple,
                cursor,
            } => (
                "screen.stream.start".into(),
                json!({"source":source,"multiple":multiple,"cursor":cursor}),
            ),
            Screen::StreamCapture { stream, timeout_ms } => (
                "screen.stream.capture".into(),
                json!({"stream":stream,"timeout_ms":timeout_ms}),
            ),
            Screen::StreamStop => ("screen.stream.stop".into(), json!({})),
        },
        Command::Clipboard { command } => match command {
            Clipboard::Read => ("clipboard.read".into(), json!({})),
            Clipboard::Write { text } => ("clipboard.write".into(), json!({"text":text.value()?})),
        },
        Command::Recipe { command } => match command {
            Recipe::List => ("recipe.list".into(), json!({})),
            Recipe::Validate { file } => (
                "recipe.validate".into(),
                json!({"recipe":serde_json::to_value(semwright_recipes::parse(&read_file(file,262144)?)?)?}),
            ),
            Recipe::Run { file, inputs } | Recipe::Test { file, inputs } => (
                "recipe.run".into(),
                json!({"recipe":serde_json::to_value(semwright_recipes::parse(&read_file(file,262144)?)?)?,"inputs":inputs.value()?}),
            ),
            Recipe::Scaffold { .. } => return Ok(None),
        },
        Command::Workflow { command } => match command {
            Workflow::Record {
                command:
                    WorkflowRecord::Start {
                        name,
                        intent,
                        capture_values,
                    },
            } => (
                "workflow.record.start".into(),
                json!({"name":name,"intent":intent,"capture_values":capture_values}),
            ),
            Workflow::Record {
                command: WorkflowRecord::Stop { failed },
            } => ("workflow.record.stop".into(), json!({"successful":!failed})),
            Workflow::Traces => ("workflow.traces.list".into(), json!({})),
            Workflow::Show { trace_id } => {
                ("workflow.trace.get".into(), json!({"trace_id":trace_id}))
            }
            Workflow::DeleteTrace { trace_id } => {
                ("workflow.trace.delete".into(), json!({"trace_id":trace_id}))
            }
            Workflow::Compile {
                name,
                trace_ids,
                description,
                parameters,
            } => (
                "workflow.compile".into(),
                json!({
                    "name":name,
                    "trace_ids":trace_ids,
                    "description":description,
                    "parameters":parameters.iter()
                        .map(|value| workflow_parameter(value))
                        .collect::<Result<Vec<_>>>()?
                }),
            ),
            Workflow::Candidate { candidate_id } => (
                "workflow.candidate.get".into(),
                json!({"candidate_id":candidate_id}),
            ),
            Workflow::DeleteCandidate { candidate_id } => (
                "workflow.candidate.delete".into(),
                json!({"candidate_id":candidate_id}),
            ),
            Workflow::Verify { candidate_id } => (
                "workflow.verify".into(),
                json!({"candidate_id":candidate_id}),
            ),
            Workflow::Replay {
                candidate_id,
                inputs,
            } => (
                "workflow.replay".into(),
                json!({"candidate_id":candidate_id,"inputs":inputs.value()?}),
            ),
            Workflow::Promote { candidate_id, slug } => (
                "workflow.promote".into(),
                json!({"candidate_id":candidate_id,"slug":slug}),
            ),
            Workflow::List => ("workflow.promotions.list".into(), json!({})),
            Workflow::Demote { slug } => ("workflow.demote".into(), json!({"slug":slug})),
        },
        Command::Plugin { command } => match command {
            Plugin::List => ("plugin.list".into(), json!({})),
            Plugin::Describe { name } => ("plugin.describe".into(), json!({"name":name})),
            Plugin::Doctor { name } => ("plugin.doctor".into(), json!({"name":name})),
            Plugin::Install { manifest } => (
                "plugin.install".into(),
                json!({"manifest":read_manifest(manifest)?}),
            ),
            Plugin::Remove { name } => ("plugin.remove".into(), json!({"name":name})),
            Plugin::Scaffold { .. } => return Ok(None),
        },
        Command::Audit {
            command: Audit::Tail { limit },
        } => ("audit.tail".into(), json!({"limit":limit})),
        Command::Watch { .. }
        | Command::Driver { .. }
        | Command::Mcp { .. }
        | Command::Config { .. }
        | Command::Completions { .. }
        | Command::Man => return Ok(None),
    };
    Ok(Some(ExecuteRequest {
        command: cmd,
        args,
        dry_run: cli.dry_run,
        backend: cli.backend.clone(),
    }))
}
pub fn print_result(value: &Value, machine: bool) -> Result<()> {
    use std::io::Write;
    let text = if machine {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    // JSON already escapes C0 controls. Escape terminal directionality controls as well.
    let mut out = std::io::stdout().lock();
    for c in text.chars() {
        if matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
            write!(out, "\\u{:04x}", c as u32)?;
        } else {
            write!(out, "{c}")?;
        }
    }
    writeln!(out)?;
    Ok(())
}
pub fn create(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn clap_definitions_are_consistent() {
        Cli::command().debug_assert();
    }
    #[test]
    fn invoke_uses_registry_command() {
        let c = Cli::try_parse_from([
            "semwright",
            "--json",
            "ui",
            "invoke",
            "ui:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--action",
            "click",
        ])
        .unwrap();
        let r = request(&c).unwrap().unwrap();
        assert_eq!(r.command, "ui.invoke");
        semwright_registry::Registry::builtin()
            .unwrap()
            .validate_input(&r.command, &r.args)
            .unwrap();
    }
    #[test]
    fn mcp_upstream_management_is_local_not_a_broker_request() {
        let c = Cli::try_parse_from([
            "semwright",
            "--json",
            "mcp",
            "upstream",
            "inspect",
            "playwright",
        ])
        .unwrap();
        assert!(request(&c).unwrap().is_none());
        assert!(matches!(
            c.command,
            Command::Mcp {
                command: Mcp::Upstream {
                    command: McpUpstream::Inspect { .. }
                }
            }
        ));
    }
    #[test]
    fn driver_management_is_local_not_a_broker_request() {
        let c = Cli::try_parse_from([
            "semwright",
            "--json",
            "driver",
            "validate",
            "/tmp/fixture-driver.json",
        ])
        .unwrap();
        assert!(request(&c).unwrap().is_none());
        assert!(matches!(
            c.command,
            Command::Driver {
                command: DriverCommand::Validate { .. }
            }
        ));
    }
    #[test]
    fn driver_distribution_commands_are_local_owner_tools() {
        for args in [
            vec![
                "semwright",
                "--json",
                "driver",
                "package",
                "inspect",
                "/tmp/fixture.swdp",
            ],
            vec![
                "semwright",
                "--json",
                "driver",
                "index",
                "validate",
                "/tmp/index.json",
            ],
            vec![
                "semwright",
                "--json",
                "--dry-run",
                "driver",
                "install",
                "/tmp/index.json",
                "fixture",
                "--application-version",
                "1",
            ],
        ] {
            let parsed = Cli::try_parse_from(args).unwrap();
            assert!(request(&parsed).unwrap().is_none());
        }
    }

    #[test]
    fn portal_start_preserves_backend_defaults_and_supports_explicit_scope() {
        let default = Cli::try_parse_from(["semwright", "portal", "start"]).unwrap();
        let default_request = request(&default).unwrap().unwrap();
        assert_eq!(default_request.command, "portal.start");
        assert_eq!(default_request.args["persist_mode"], 0);
        assert_eq!(default_request.args["clipboard"], false);
        assert!(default_request.args.get("keyboard").is_none());
        assert!(default_request.args.get("pointer").is_none());

        let scoped = Cli::try_parse_from([
            "semwright",
            "portal",
            "start",
            "--keyboard=false",
            "--pointer",
            "--persist-mode",
            "2",
            "--clipboard",
        ])
        .unwrap();
        let scoped_request = request(&scoped).unwrap().unwrap();
        assert_eq!(scoped_request.args["keyboard"], false);
        assert_eq!(scoped_request.args["pointer"], true);
        assert_eq!(scoped_request.args["persist_mode"], 2);
        assert_eq!(scoped_request.args["clipboard"], true);
        semwright_registry::Registry::builtin()
            .unwrap()
            .validate_input(&scoped_request.command, &scoped_request.args)
            .unwrap();

        let clear = Cli::try_parse_from(["semwright", "portal", "restore-clear"]).unwrap();
        assert_eq!(
            request(&clear).unwrap().unwrap().command,
            "portal.restore.clear"
        );
    }
    #[test]
    fn no_permission_upgrade_flags() {
        assert!(Cli::try_parse_from(["semwright", "--approve", "doctor"]).is_err());
    }
    #[test]
    fn known_examples_match_schemas() {
        for args in [
            vec!["semwright", "doctor"],
            vec!["semwright", "window", "list"],
            vec![
                "semwright",
                "ui",
                "find",
                "--role",
                "button",
                "--name",
                "Save",
            ],
            vec!["semwright", "ui", "snapshot", "--actionable"],
        ] {
            let c = Cli::try_parse_from(args).unwrap();
            let r = request(&c).unwrap().unwrap();
            semwright_registry::Registry::builtin()
                .unwrap()
                .validate_input(&r.command, &r.args)
                .unwrap();
        }
    }
}
