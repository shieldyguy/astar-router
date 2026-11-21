//! The command: what it exits with and what it says.

use std::process::Command;

fn fixture(name: &str) -> String {
    format!("{}/tests/scenes/{}", env!("CARGO_MANIFEST_DIR"), name)
}

/// Run astar with these arguments; a plan path is added when `plan` is true. (exit code, stderr, the plan)
fn astar(args: &[&str], plan: bool) -> (i32, String, String) {
    let out = std::env::temp_dir().join(format!("astar-cli-{}-{:?}.json", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_file(&out);
    let mut c = Command::new(env!("CARGO_BIN_EXE_astar"));
    let mut args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    if plan {
        // the plan goes right after the scene
        let at = args.iter().position(|a| a.ends_with(".json") || a.starts_with('/')).map_or(args.len(), |i| i + 1);
        args.insert(at, out.to_string_lossy().into_owned());
    }
    let o = c.args(&args).output().expect("astar runs");
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    (o.status.code().expect("astar exits, it is not killed"), String::from_utf8_lossy(&o.stderr).into_owned(), text)
}

#[test]
fn exit_0_whole_1_open_2_bad_input_3_narrowed() {
    // whole
    let (code, err, plan) = astar(&["route", &fixture("tiny11.json")], true);
    assert_eq!(code, 0, "{}", err);
    assert!(err.contains("1 of 1 nets joined") && plan.contains("\"left_out\": []"));
    // nets open (the drops left out: the ground net's pads never reach its plane)
    let (code, err, plan) = astar(&["route", &fixture("every_field.json"), "--skip", "drops"], true);
    assert_eq!(code, 1, "{}", err);
    assert!(err.contains("passes left out: drops") && plan.contains("\"kind\": \"open\", \"net\": \"GND\""), "{}", err);
    // the old form says the same and exits 0 for it, as it always did
    let (code, err, plan) = astar(&[&fixture("every_field.json"), "--skip", "drops"], true);
    assert_eq!(code, 0, "{}", err);
    assert!(plan.contains("\"kind\": \"open\", \"net\": \"GND\""));
    // a width narrowed: 0 unless --strict, then 3
    let (code, err, plan) = astar(&["route", &fixture("every_field.json")], true);
    assert_eq!(code, 0, "{}", err);
    assert!(err.contains("drawn narrower") && plan.contains("\"kind\": \"narrowed\", \"net\": \"VCC\""), "{}", err);
    let (code, _, _) = astar(&["route", &fixture("every_field.json"), "--strict"], true);
    assert_eq!(code, 3);
}

#[test]
fn bad_input_is_exit_2_with_the_reason_and_no_plan() {
    for (args, says) in [
        (vec!["route".to_string(), "/no/such/scene.json".to_string()], "cannot read"),
        (vec!["route".to_string(), fixture("../same.sh")], "at byte"),
        (vec!["route".to_string(), fixture("every_field.json"), "--pitch".to_string(), "0.001".to_string()], "the pitch is 0.001 mm"),
        (vec!["route".to_string(), fixture("every_field.json"), "--pitch".to_string(), "0.01".to_string(), "--via".to_string(), "1e9".to_string()], "--via is 1000000000"),
        (vec!["route".to_string(), fixture("every_field.json"), "--skip".to_string(), "nope".to_string()], "--skip takes the name of a pass"),
        (vec!["route".to_string(), fixture("every_field.json"), "--frobnicate".to_string()], "usage:"),
    ] {
        let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let (code, err, plan) = astar(&a, true);
        assert_eq!(code, 2, "{:?}: {}", args, err);
        assert!(err.contains(says), "{:?}: {}", args, err);
        assert!(plan.is_empty(), "{:?} wrote a plan", args);
    }
}

#[test]
fn the_passes_are_listed_in_order() {
    let o = Command::new(env!("CARGO_BIN_EXE_astar")).arg("--passes").output().unwrap();
    let names: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(|l| l.split_whitespace().next().unwrap_or("").to_string()).collect();
    assert_eq!(names, ["pour", "fences", "exits", "cross", "drops", "route", "tight", "arcs"]);
}
