use svtoggle_core::firewall::Fw;
use svtoggle_core::{refresh, toggle};

const USAGE: &str = "usage: svtoggle <overwatch|deadlock> <block|allow|status|refresh> [region]\n       svtoggle protect-check | monitor";

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&a) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(a: &[String]) -> Result<(), String> {
    match a.first().map(String::as_str) {
        Some("monitor") => return monitor(),
        Some("protect-check") => {
            let bad = refresh::protect_problems();
            if bad.is_empty() {
                println!("protect.txt OK ({} entries)", refresh::user_protect().len());
            }
            bad.iter().for_each(|l| println!("{l}"));
            return Ok(());
        }
        _ => {}
    }
    let (game, cmd) = match a {
        [g, c, ..] => (g.as_str(), c.as_str()),
        _ => return Err(USAGE.into()),
    };
    let profs = refresh::load_all();
    let prof = profs.iter().find(|p| p.game == game).ok_or(USAGE)?;
    if cmd == "refresh" {
        let (new, msg) = refresh::refresh(game, prof)?;
        println!("{msg}");
        let fix = format!("blocked rules were NOT re-applied; run `svtoggle {game} refresh` as administrator");
        match Fw::open() {
            Err(e) => println!("{fix} (firewall: {})", toggle::deny(e)),
            Ok(fw) => {
                let errs = toggle::reconcile(&fw, Some(prof), &new);
                if errs.is_empty() {
                    println!("blocked regions re-applied with the new lists");
                } else {
                    errs.iter().for_each(|e| println!("re-apply failed: {e}"));
                    if errs.iter().any(|e| e.contains("access denied")) {
                        println!("{fix}");
                    }
                }
            }
        }
        return Ok(());
    }
    let fw = Fw::open().map_err(|e| e.to_string())?;
    match (cmd, a.get(2)) {
        ("status", None) => {
            for r in &prof.regions {
                let b = toggle::is_blocked(&fw, prof, &r.id);
                println!("{:<12} {:<8} {}", r.id, if b { "BLOCKED" } else { "allowed" }, r.name);
            }
        }
        ("block" | "allow", Some(region)) => println!("{}", toggle::set(&fw, prof, region, cmd == "block")?),
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// Debug hook: `svtoggle monitor` prints what the "now connected" monitor sees, once a second.
fn monitor() -> Result<(), String> {
    use svtoggle_core::{cidr, monitor::{Monitor, Target}};
    let mut targets = Vec::new();
    for p in refresh::load_all() {
        for r in &p.regions {
            for c in cidr::parse_all(&r.cidrs)? {
                targets.push(Target { cidr: c, game: p.game.clone(), region: r.id.clone() });
            }
        }
    }
    let m = Monitor::start(targets, || {});
    loop {
        println!("games={:?} current={:?} error={:?}", m.running_games(), m.current(), m.error());
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
