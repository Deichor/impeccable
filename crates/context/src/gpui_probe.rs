//! `impeccable gpui`: the agent's side of `impeccable-gpui-probe`, the
//! debug-build bridge inside a running GPUI app. The probe serves a
//! localhost API and writes its port to `.impeccable/gpui-probe.json` in the
//! app's working directory.

use std::time::{Duration, Instant};

use impeccable_common::Io;
use serde_json::{json, Value};

use crate::jsp;

const USAGE: &str = "Usage: impeccable gpui <command> [--port N]

Talks to a running GPUI app built with impeccable-gpui-probe (debug builds).

  status                         windows, appearance hook, current pick
  capture --out <dir> [--sizes 960x640,1440x900] [--appearances dark,light]
                                 one PNG per appearance x size, then restores the size
  inspector --open | --close     show or hide the inspector panel
  pick --x <x> --y <y>           pick the element at a window point (logical px)
  selection [--wait] [--after <seq>]
                                 the picked element: source line, style, bounds
  variants --file <json>         {\"variants\":[{\"label\":..,\"style\":{..patch..}}]}
  apply --index <n> | --original show one variant, or the original style
  decision [--wait]              what the user accepted (ctrl-alt-enter in the app)
";

const WAIT_LIMIT: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_millis(400);

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// `--port`, else `.impeccable/gpui-probe.json` here or in a parent.
fn port(args: &[String], cwd: &str) -> Result<u16, String> {
    if let Some(p) = flag(args, "--port") {
        return p.parse().map_err(|_| format!("invalid --port {p}"));
    }
    let mut dir = jsp::resolve(cwd, &[]);
    loop {
        let file = jsp::join(&[&dir, ".impeccable", "gpui-probe.json"]);
        if let Some(v) = crate::util::read_json(&file) {
            if let Some(p) = v.get("port").and_then(Value::as_u64) {
                return Ok(p as u16);
            }
        }
        let parent = jsp::dirname(&dir);
        if parent == dir {
            break;
        }
        dir = parent;
    }
    Err("no running probe found: start the app's debug build (it writes .impeccable/gpui-probe.json) or pass --port".into())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(40)).build()
}

fn get(port: u16, path: &str) -> Result<ureq::Response, String> {
    match agent().get(&format!("http://127.0.0.1:{port}{path}")).call() {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(_, r)) => Err(r.into_string().unwrap_or_default()),
        Err(e) => Err(format!("probe on port {port} did not answer: {e}")),
    }
}

fn post(port: u16, path: &str, body: &Value) -> Result<Value, String> {
    match agent().post(&format!("http://127.0.0.1:{port}{path}")).send_string(&body.to_string()) {
        Ok(r) => r.into_json().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(_, r)) => Err(r.into_string().unwrap_or_default()),
        Err(e) => Err(format!("probe on port {port} did not answer: {e}")),
    }
}

fn get_json(port: u16, path: &str) -> Result<Value, String> {
    get(port, path)?.into_json().map_err(|e| e.to_string())
}

fn parse_size(s: &str) -> Option<(f64, f64)> {
    let (w, h) = s.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn capture(port: u16, args: &[String], cwd: &str) -> Result<Value, String> {
    let out = jsp::resolve(cwd, &[flag(args, "--out").unwrap_or(".impeccable/review")]);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let sizes: Vec<(f64, f64)> = match flag(args, "--sizes") {
        Some(list) => list.split(',').map(|s| parse_size(s).ok_or(format!("invalid size {s}"))).collect::<Result<_, _>>()?,
        None => vec![],
    };
    let appearances: Vec<&str> = flag(args, "--appearances").map(|a| a.split(',').collect()).unwrap_or_default();
    let status = get_json(port, "/status")?;
    let window = status["windows"].get(0).cloned().unwrap_or(Value::Null);
    let original = (window["width"].as_f64(), window["height"].as_f64());
    let mut files = Vec::new();
    let appearance_list: Vec<Option<&str>> =
        if appearances.is_empty() { vec![None] } else { appearances.iter().map(|a| Some(*a)).collect() };
    let size_list: Vec<Option<(f64, f64)>> = if sizes.is_empty() { vec![None] } else { sizes.iter().map(|s| Some(*s)).collect() };
    for appearance in &appearance_list {
        if let Some(a) = appearance {
            let dark = match *a {
                "dark" => true,
                "light" => false,
                other => return Err(format!("unknown appearance {other} (dark or light)")),
            };
            post(port, "/appearance", &json!({ "dark": dark }))?;
        }
        for size in &size_list {
            if let Some((w, h)) = size {
                post(port, "/resize", &json!({ "width": w, "height": h }))?;
            }
            let png = get(port, "/capture")?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut png.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
            let name = match (appearance, size) {
                (Some(a), Some((w, h))) => format!("window-{a}-{w}x{h}.png"),
                (Some(a), None) => format!("window-{a}.png"),
                (None, Some((w, h))) => format!("window-{w}x{h}.png"),
                (None, None) => "window.png".to_string(),
            };
            let path = jsp::join(&[&out, &name]);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            files.push(Value::String(path));
        }
    }
    if let (Some(w), Some(h)) = original {
        if !sizes.is_empty() {
            let _ = post(port, "/resize", &json!({ "width": w, "height": h }));
        }
    }
    Ok(json!({ "files": files }))
}

fn wait_for(port: u16, path: &str, ready: impl Fn(&Value) -> bool) -> Result<Value, String> {
    let started = Instant::now();
    loop {
        let v = get_json(port, path)?;
        if ready(&v) {
            return Ok(v);
        }
        if started.elapsed() > WAIT_LIMIT {
            return Err(format!("nothing arrived from {path} within {}s", WAIT_LIMIT.as_secs()));
        }
        std::thread::sleep(POLL);
    }
}

fn dispatch(sub: &str, args: &[String], cwd: &str) -> Result<Value, String> {
    let port = port(args, cwd)?;
    match sub {
        "status" => get_json(port, "/status"),
        "capture" => capture(port, args, cwd),
        "inspector" => post(port, "/inspector", &json!({ "open": !has(args, "--close") })),
        "pick" => {
            let x: f64 = flag(args, "--x").and_then(|v| v.parse().ok()).ok_or("pick needs --x")?;
            let y: f64 = flag(args, "--y").and_then(|v| v.parse().ok()).ok_or("pick needs --y")?;
            post(port, "/pick", &json!({ "x": x, "y": y }))
        }
        "selection" => {
            if !has(args, "--wait") {
                return get_json(port, "/selection");
            }
            let after = flag(args, "--after").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            wait_for(port, "/selection", |v| {
                !v["picking"].as_bool().unwrap_or(false) && v["source"].is_string() && v["seq"].as_u64().unwrap_or(0) > after
            })
        }
        "variants" => {
            let file = flag(args, "--file").ok_or("variants needs --file <json>")?;
            let text = std::fs::read_to_string(jsp::resolve(cwd, &[file])).map_err(|e| format!("{file}: {e}"))?;
            let body: Value = serde_json::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
            post(port, "/variants", &body)
        }
        "apply" => {
            let index = if has(args, "--original") {
                Value::Null
            } else {
                Value::from(flag(args, "--index").and_then(|v| v.parse::<u64>().ok()).ok_or("apply needs --index <n> or --original")?)
            };
            post(port, "/apply", &json!({ "index": index }))
        }
        "decision" => {
            if has(args, "--wait") {
                wait_for(port, "/decision", |v| !v["decision"].is_null())
            } else {
                get_json(port, "/decision")
            }
        }
        other => Err(format!("unknown gpui command `{other}`\n\n{USAGE}")),
    }
}

pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some(sub) = args.first().map(String::as_str).filter(|s| !s.starts_with('-')) else {
        io.out(USAGE);
        return if args.iter().any(|a| a == "--help" || a == "-h") { 0 } else { 1 };
    };
    let cwd = io.cwd.to_string_lossy().into_owned();
    match dispatch(sub, &args[1..], &cwd) {
        Ok(v) => {
            io.out(&format!("{}\n", serde_json::to_string_pretty(&v).unwrap_or_default()));
            0
        }
        Err(e) => {
            io.err(&format!("impeccable gpui {sub}: {e}\n"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("960x640"), Some((960.0, 640.0)));
        assert_eq!(parse_size("wide"), None);
    }

    #[test]
    fn port_from_discovery_file_in_a_parent() {
        let root = std::env::temp_dir().join(format!("impeccable-gpui-port-{}", std::process::id()));
        let nested = root.join("app").join("src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(root.join(".impeccable")).unwrap();
        std::fs::write(root.join(".impeccable").join("gpui-probe.json"), "{\"port\":4123}").unwrap();
        assert_eq!(port(&[], &nested.to_string_lossy()), Ok(4123));
        assert_eq!(port(&["--port".into(), "99".into()], "/"), Ok(99));
        let _ = std::fs::remove_dir_all(&root);
    }
}
