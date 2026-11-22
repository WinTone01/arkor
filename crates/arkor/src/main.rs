mod ui;

use arkor_core::GameId;

const USAGE: &str = "\
usage:
  arkor                         open the library window
  arkor open <global|import|store:id>  open the window on a page
  arkor list                    print installed games from every source
  arkor launch <store:id>       start a game through its store with its profile
  arkor import <prefix> [exe]   list games found in a Wine prefix, or add one
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
        Some("import") => import(
            args.get(1).ok_or_else(|| anyhow::anyhow!(USAGE))?,
            args.get(2),
        ),
        Some("launch") => {
            let id: GameId = args.get(1).ok_or_else(|| anyhow::anyhow!(USAGE))?.parse()?;
            match arkor_launch::start::start(&id) {
                Ok(never) => match never {},
                Err(e) => Err(e),
            }
        }
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
    let (games, errors) = arkor_stores::all_games();
    for game in &games {
        println!("{:<42} {}", game.id.to_string(), game.name);
    }
    for e in errors {
        eprintln!("warning: {e:#}");
    }
    Ok(())
}

fn import(prefix_arg: &str, exe: Option<&String>) -> anyhow::Result<()> {
    use arkor_stores::prefix;
    let root = prefix::resolve(std::path::Path::new(prefix_arg)).ok_or_else(|| {
        anyhow::anyhow!("not a Wine prefix (no drive_c folder found): {prefix_arg}")
    })?;
    let Some(exe) = exe else {
        for c in prefix::find_executables(&root) {
            let size = if c.size > 0 {
                prefix::human_size(c.size)
            } else {
                String::new()
            };
            println!("{size:>8}  {}", c.path.display());
        }
        return Ok(());
    };
    let exe = std::path::PathBuf::from(exe);
    anyhow::ensure!(exe.is_file(), "{} not found", exe.display());
    let mut imported = arkor_core::imported::Imported::load()?;
    let id = imported.add(prefix::suggested_name(&exe), root, exe);
    imported.save()?;
    println!("added local:{id}");
    Ok(())
}
