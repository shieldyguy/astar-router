//! Only the nets asked for are routed: by the settings (`--nets`, `--except`) or by the scene's `scope`.
//! A net not asked for gets no copper, is not counted as open, and is named in the plan (`not_asked`),
//! so that astar's own check does not ask whether it is whole.
//!
//! The scene is tests/scenes/old_copper.json: A (a stub to be joined on), B (two pads and a loose piece),
//! G (a plane's net with a pad that needs its via).

use astar::json::J;
use astar::{route, Cfg, Scene};

fn scene() -> String {
    format!("{}/tests/scenes/old_copper.json", env!("CARGO_MANIFEST_DIR"))
}

fn new_copper(plan: &J) -> Vec<String> {
    plan.get("nets").arr().iter().filter(|n| !n.get("tracks").arr().is_empty() || !n.get("vias").arr().is_empty()).map(|n| n.get("name").s().unwrap().to_string()).collect()
}

fn names(j: &J) -> Vec<String> {
    j.arr().iter().map(|x| x.s().unwrap().to_string()).collect()
}

#[test]
fn only_the_nets_asked_for_get_copper() {
    let cfg = Cfg { nets: vec!["B".into()], ..Cfg::default() };
    let routed = route(Scene::load(&scene()).unwrap(), &cfg).unwrap();
    assert!(routed.open_nets().is_empty(), "a net not asked for is not open");
    assert_eq!(routed.exit_code(false), 0);
    let plan = astar::json::parse(&routed.plan()).unwrap();
    assert_eq!(new_copper(&plan), ["B"]);
    assert_eq!(names(plan.get("not_asked")), ["A", "G"]);
    assert!(plan.get("notes").arr().iter().any(|n| n.get("kind").s() == Some("not asked")));
}

#[test]
fn a_net_named_as_an_exception_is_left_as_it_is() {
    let cfg = Cfg { except: vec!["A".into(), "G".into()], ..Cfg::default() };
    let plan = astar::json::parse(&route(Scene::load(&scene()).unwrap(), &cfg).unwrap().plan()).unwrap();
    assert_eq!(new_copper(&plan), ["B"]);
}

#[test]
fn a_star_stands_for_any_run_of_letters() {
    assert!(astar::like("SPI_*", "SPI_MOSI") && astar::like("*", "GND") && astar::like("*_P", "USB_D_P") && astar::like("A*B*C", "AxxBC"));
    assert!(!astar::like("SPI_*", "SPI") && !astar::like("CLK", "CLK2") && !astar::like("*_P", "USB_N"));
    let cfg = Cfg { nets: vec!["?".into()], ..Cfg::default() };
    let e = route(Scene::load(&scene()).unwrap(), &cfg).err().expect("no net is named ?");
    assert!(e.to_string().contains("--nets ?: the board has no net of that name"), "{}", e);
}

/// The scene's own `scope` does the same, and the settings can only narrow it.
#[test]
fn the_scenes_own_scope_is_kept_to() {
    let text = std::fs::read_to_string(scene()).unwrap().replacen("\"units\": \"mm\",", "\"units\": \"mm\", \"scope\": {\"nets\": [\"A\", \"B\"]},", 1);
    assert!(text.contains("\"scope\""));
    let dir = std::env::temp_dir().join(format!("astar-scope-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (file, out) = (dir.join("scene.json"), dir.join("plan.json"));
    std::fs::write(&file, text).unwrap();
    let plan = astar::json::parse(&route(Scene::load(file.to_str().unwrap()).unwrap(), &Cfg::default()).unwrap().plan()).unwrap();
    assert_eq!(new_copper(&plan), ["A", "B"]);
    assert_eq!(names(plan.get("not_asked")), ["G"]);
    let narrowed = astar::json::parse(&route(Scene::load(file.to_str().unwrap()).unwrap(), &Cfg { except: vec!["A".into()], ..Cfg::default() }).unwrap().plan()).unwrap();
    assert_eq!(new_copper(&narrowed), ["B"]);
    // the run by its command: every net asked for joined is exit 0, and the check agrees
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_astar")).args(args).output().unwrap();
    let routed = run(&["route", file.to_str().unwrap(), out.to_str().unwrap(), "--nets", "B"]);
    assert_eq!(routed.status.code(), Some(0), "{}", String::from_utf8_lossy(&routed.stderr));
    assert!(String::from_utf8_lossy(&routed.stderr).contains("1 of 1 nets joined (2 not asked for)"), "{}", String::from_utf8_lossy(&routed.stderr));
    let checked = run(&["check", file.to_str().unwrap(), out.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(checked.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&checked.stdout), String::from_utf8_lossy(&checked.stderr));
}
