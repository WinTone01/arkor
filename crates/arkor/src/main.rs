mod ui;

use arkor_core::GameId;

const USAGE: &str = "\
usage:
  arkor                         open the library window
  arkor open <global|store:id>  open the window on a page
  arkor list                    print installed games
  arkor run <store:id> -- CMD   launch CMD with the game's profile
                                (Steam launch options: arkor run steam:<appid> -- %command%)";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => ui::run(None),
        Some("open") => ui::run(Some(
            args.get(1).ok_or_else(|| anyhow::anyhow!(USAGE))?.clone(),
        )),
        Some("list") => list(),
        Some("run") => {
            let id: GameId = args.get(1).ok_or_else(|| anyhow::anyhow!(USAGE))?.parse()?;
            let command = match args.get(2).map(String::as_str) {
                Some("--") => &args[3..],
                _ => &args[2..],
            };
            match arkor_launch::exec(&id, command) {
                Ok(never) => match never {},
                Err(e) => Err(e),
            }
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command `{other}`\n{USAGE}"),
    }
}

fn list() -> anyhow::Result<()> {
    let (games, errors) = arkor_stores::steam::installed_games();
    for game in &games {
        println!("{:<16} {}", game.id.to_string(), game.name);
    }
    for e in errors {
        eprintln!("warning: {e:#}");
    }
    Ok(())
}
