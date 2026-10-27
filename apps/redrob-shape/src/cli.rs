//! The command line: `redrob-shape <command>` works on scene files without opening a window.
//!
//! ```text
//! redrob-shape convert IN OUT                       any readable format to any writable one
//! redrob-shape info FILE                            the scene summary as JSON
//! redrob-shape tools                                the tool catalogue (names, parameters) as JSON
//! redrob-shape tool [--in FILE] [--out FILE] NAME [JSON]
//!                                                   one tool call, its JSON result on stdout
//! redrob-shape run [--in FILE] [--out FILE] SCRIPT|-
//!                                                   a Shape Script, one result line per statement
//! redrob-shape mcp --headless [--in FILE] [--save FILE]
//!                                                   MCP on stdio against a scene in this process
//! redrob-shape mcp --connect ADDR                   MCP on stdio against a running window
//! ```
//!
//! Without `--in` a command starts from an empty scene. `--out` / `--save` write the scene after
//! the command, in the format the extension picks. Exit codes: 0 ok, 1 the command failed,
//! 2 a usage error.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use shape_agent::{script, tools};
use shape_scene::Scene;

const USAGE: &str = "\
usage:
  redrob-shape convert IN OUT
  redrob-shape info FILE
  redrob-shape tools
  redrob-shape tool [--in FILE] [--out FILE] NAME [JSON]
  redrob-shape run [--in FILE] [--out FILE] SCRIPT|-
  redrob-shape mcp --headless [--in FILE] [--save FILE]
  redrob-shape mcp --connect 127.0.0.1:PORT
  redrob-shape [--demo] [--no-grid] [--shading wire|solid|material] [FILE]   (opens the window)";

const COMMANDS: [&str; 7] = ["convert", "info", "tools", "tool", "run", "mcp", "help"];

/// Runs the command line when the first argument is a command and returns the exit code;
/// `None` opens the window.
pub fn dispatch(args: &[String]) -> Option<i32> {
    let cmd = args.first()?;
    if !COMMANDS.contains(&cmd.as_str()) && cmd != "--help" && cmd != "-h" {
        return None;
    }
    // `mcp` talks over stdio pipes its parent set up; only a person's terminal needs the console.
    if cmd != "mcp" {
        attach_console();
    }
    let rest = &args[1..];
    let result = match cmd.as_str() {
        "convert" => convert(rest),
        "info" => info(rest),
        "tools" => {
            println!("{}", pretty(&tools::tool_definitions()));
            Ok(())
        }
        "tool" => tool(rest),
        "run" => run(rest),
        "mcp" => mcp(rest),
        _ => {
            println!("{USAGE}");
            return Some(0);
        }
    };
    Some(match result {
        Ok(()) => 0,
        Err(Failure::Usage(m)) => {
            eprintln!("redrob-shape {cmd}: {m}\n\n{USAGE}");
            2
        }
        Err(Failure::Failed(m)) => {
            eprintln!("redrob-shape {cmd}: {m}");
            1
        }
    })
}

enum Failure {
    Usage(String),
    Failed(String),
}

type R = Result<(), Failure>;

fn usage(m: impl Into<String>) -> Failure {
    Failure::Usage(m.into())
}

fn failed(m: impl Into<String>) -> Failure {
    Failure::Failed(m.into())
}

/// `--flag VALUE` options and the remaining positional arguments, in order.
struct Args {
    flags: Vec<(String, String)>,
    switches: Vec<String>,
    positional: Vec<String>,
}

impl Args {
    fn parse(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Self, Failure> {
        let mut out = Args { flags: Vec::new(), switches: Vec::new(), positional: Vec::new() };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if with_value.contains(&a.as_str()) {
                let v = it.next().ok_or_else(|| usage(format!("{a} needs a value")))?;
                out.flags.push((a.clone(), v.clone()));
            } else if switches.contains(&a.as_str()) {
                out.switches.push(a.clone());
            } else if a.starts_with("--") {
                return Err(usage(format!("unknown option {a}")));
            } else {
                out.positional.push(a.clone());
            }
        }
        Ok(out)
    }

    fn get(&self, flag: &str) -> Option<PathBuf> {
        self.flags.iter().rev().find(|(f, _)| f == flag).map(|(_, v)| PathBuf::from(v))
    }

    fn has(&self, switch: &str) -> bool {
        self.switches.iter().any(|s| s == switch)
    }
}

fn open(path: &Path) -> Result<Scene, Failure> {
    shape_format::import(path).map_err(|e| failed(format!("can't open {}: {e}", path.display())))
}

fn save(scene: &Scene, path: &Path) -> R {
    shape_format::export(scene, path).map_err(|e| failed(format!("can't write {}: {e}", path.display())))
}

/// The scene `--in` names, or an empty one.
fn start(a: &Args) -> Result<Scene, Failure> {
    match a.get("--in") {
        Some(p) => open(&p),
        None => Ok(Scene::new("Scene")),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn convert(args: &[String]) -> R {
    let a = Args::parse(args, &[], &[])?;
    let [input, output] = a.positional.as_slice() else {
        return Err(usage("convert needs IN and OUT"));
    };
    let scene = open(Path::new(input))?;
    save(&scene, Path::new(output))?;
    println!("{}", json!({"input": input, "output": output}));
    Ok(())
}

fn info(args: &[String]) -> R {
    let a = Args::parse(args, &[], &[])?;
    let [file] = a.positional.as_slice() else {
        return Err(usage("info needs FILE"));
    };
    let mut scene = open(Path::new(file))?;
    let summary = tools::execute(&mut scene, "scene_summary", &json!({})).map_err(failed)?;
    println!("{}", pretty(&summary));
    Ok(())
}

fn tool(args: &[String]) -> R {
    let a = Args::parse(args, &["--in", "--out"], &[])?;
    let (name, params) = match a.positional.as_slice() {
        [name] => (name, json!({})),
        [name, params] => (name, serde_json::from_str(params).map_err(|e| usage(format!("JSON: {e}")))?),
        _ => return Err(usage("tool needs NAME and optionally JSON")),
    };
    let mut scene = start(&a)?;
    let result = tools::execute(&mut scene, name, &params).map_err(failed)?;
    println!("{}", pretty(&result));
    if let Some(out) = a.get("--out") {
        save(&scene, &out)?;
    }
    Ok(())
}

fn run(args: &[String]) -> R {
    let a = Args::parse(args, &["--in", "--out"], &[])?;
    let [source] = a.positional.as_slice() else {
        return Err(usage("run needs SCRIPT (a file, or - for stdin)"));
    };
    let text = if source == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| failed(format!("can't read stdin: {e}")))?;
        s
    } else {
        std::fs::read_to_string(source).map_err(|e| failed(format!("can't read {source}: {e}")))?
    };
    let mut scene = start(&a)?;
    let (lines, _) = script::run(&mut scene, &text);
    // A statement that failed is reported as `line N: …` (script::run); the run fails with it.
    let mut errors = 0;
    for line in &lines {
        if line.starts_with("line ") && line.contains(": ") {
            errors += 1;
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
    if let Some(out) = a.get("--out") {
        save(&scene, &out)?;
    }
    if errors > 0 {
        return Err(failed(format!("{errors} statement(s) failed")));
    }
    Ok(())
}

fn mcp(args: &[String]) -> R {
    let a = Args::parse(args, &["--connect", "--in", "--save"], &["--headless"])?;
    if let Some(addr) = a.flags.iter().find(|(f, _)| f == "--connect").map(|(_, v)| v.clone()) {
        if a.has("--headless") {
            return Err(usage("--connect and --headless are exclusive"));
        }
        return shape_agent::serve_stdio(&addr).map_err(|e| failed(e.to_string()));
    }
    if !a.has("--headless") {
        return Err(usage("mcp needs --headless or --connect ADDR"));
    }
    let scene = start(&a)?;
    shape_agent::serve_stdio_headless(scene, a.get("--save")).map_err(|e| failed(e.to_string()))
}

/// The release build on Windows is a GUI program with no console of its own, so its output would
/// go nowhere; the command line writes to the console it was started from instead.
#[cfg(windows)]
fn attach_console() {
    let _ = winsafe::AttachConsole(winsafe::PidParent::Parent);
}

#[cfg(not(windows))]
fn attach_console() {}
