//! A scene routed through the library, in ten lines:  cargo run --release --example route -- SCENE.json PLAN.json
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let scene = astar::Scene::load(&args[1])?;
    let routed = astar::route(scene, &astar::Cfg::default())?;
    for (net, joined, pads) in routed.open_nets() {
        println!("open: {} ({} of {} pads)", net, joined, pads);
    }
    std::fs::write(&args[2], routed.plan())?;
    Ok(println!("{} notes, {:.3} s", routed.router.notes.len(), routed.seconds))
}
