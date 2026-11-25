mod ui;

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use arkor_core::GameId;
use arkor_core::library::{GameKind, Library, NewGame};

const USAGE: &str = "\
usage:
  arkor                                   open the window
  arkor --tray                            start in the system tray (used by autostart)
  arkor --console                         open in console mode (full screen, gamepad)
  arkor open <global|add|stores|protons|id>
                                          open the window on a page
  arkor list                              print the library
  arkor system                            machine info and tool status
  arkor detect                            print games found in other launchers
  arkor add <exe> [--name N] [--prefix P] [--linux] [-- ARGS...]
                                          add a game by hand
  arkor add-detected <store:id>           add a game listed by `detect`
  arkor import <prefix> [exe]             list games in a Wine prefix, or add one
  arkor rename <id> <name>                rename a library game
  arkor remove <id>                       remove a game from the library (files stay)
  arkor launch <id>                       start a game with its profile
  arkor run <id> -- CMD                   run CMD with the game's profile
                                          (Steam launch options: arkor run <id> -- %command%)
  arkor tool <id> winecfg | winetricks [VERBS] | run <file>
                                          prefix tools
  arkor kill <id>                         stop everything in a game's prefix
  arkor proton installed | list <ge|cachyos|dw|wineland> | install <variant> <tag>
                                          Proton versions
  arkor optiscaler download [--edge] | install <id> <dir> [proxy] | remove <id>
                  | optipatcher <id> [remove] | fsr4 <id> <dll|remove>
                                          OptiScaler
  arkor dlss-enabler download | install <id> <dir> | remove <id>
                                          DLSS Enabler
  arkor reshade download | install <id> <dir> [proxy] [--32] | remove <id>
                                          ReShade
  arkor streamline download | install <id> <game dir> | remove <id>
                                          newer Streamline / DLSS DLLs
  arkor shortcut <id> menu | desktop | steam | remove | command
                                          launch the game from elsewhere
  arkor bundle download <helixsr|dlssnr-rdna4|dlssnr-rdna3>
             | helixsr <game dir> [remove]   (opens HelixSR's setup in a terminal)
             | dlssnr <id> <game dir> <nvngx_dlssnr file> [model] | dlssnr-remove <id>
                                          AMD upscaler bundles (HelixSR, DLSS-NR)
  arkor update [--check]                  update Arkor itself from its GitHub releases
  arkor layer list | install <vksumi|vknemu|mako> | remove <id>
                                          extra Vulkan layers (user folder)
  arkor backup create [--with-prefixes] | restore <file>
                                          back up / restore Arkor's data";

fn arg(args: &[String], ix: usize) -> anyhow::Result<&String> {
    args.get(ix).ok_or_else(|| anyhow!(USAGE))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => ui::run(None, false),
        Some("--tray") => ui::run(None, true),
        Some("--console") => ui::run(Some("console".into()), false),
        Some("open") => ui::run(Some(arg(&args, 1)?.clone()), false),
        Some("list") => list(),
        Some("system") => {
            for (label, value) in arkor_launch::tools::system_info() {
                println!("{label:<12} {value}");
            }
            println!();
            for t in arkor_launch::tools::tool_status() {
                match t.found {
                    Some(v) => println!("✓ {:<28} {v}", t.name),
                    None => println!("✗ {:<28} sudo pacman -S {}", t.name, t.package),
                }
            }
            Ok(())
        }
        Some("detect") => detect(),
        Some("add") => add(&args[1..]),
        Some("add-detected") => add_detected(arg(&args, 1)?),
        Some("import") => import(arg(&args, 1)?, args.get(2)),
        Some("rename") => {
            let id: GameId = arg(&args, 1)?.parse()?;
            let mut library = Library::load()?;
            library.rename(&id.id, arg(&args, 2)?)?;
            library.save()?;
            println!("renamed {id}");
            Ok(())
        }
        Some("remove") => {
            let id: GameId = arg(&args, 1)?.parse()?;
            let mut library = Library::load()?;
            anyhow::ensure!(
                library.get(&id.id).is_some(),
                "`{id}` is not in the library"
            );
            library.remove(&id.id);
            library.save()?;
            println!("removed {id} (game files and prefix untouched)");
            Ok(())
        }
        Some("launch") => {
            let id: GameId = arg(&args, 1)?.parse()?;
            std::process::exit(arkor_launch::start::start(&id)?);
        }
        Some("run") => {
            let id: GameId = arg(&args, 1)?.parse()?;
            let command = match args.get(2).map(String::as_str) {
                Some("--") => &args[3..],
                _ => &args[2..],
            };
            std::process::exit(arkor_launch::exec(&id, command)?);
        }
        Some("tool") => tool(&args[1..]),
        Some("kill") => arkor_launch::start::kill_prefix(&arg(&args, 1)?.parse()?),
        Some("proton") => proton(&args[1..]),
        Some("optiscaler") => optiscaler(&args[1..]),
        Some("dlss-enabler") => dlss_enabler(&args[1..]),
        Some("reshade") => reshade(&args[1..]),
        Some("backup") => backup(&args[1..]),
        Some("layer") => layer(&args[1..]),
        Some("update") => self_update(args.get(1).is_some_and(|a| a == "--check")),
        Some("shortcut") => shortcut(&args[1..]),
        Some("bundle") => bundle(&args[1..]),
        Some("streamline") => streamline(&args[1..]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command `{other}`\n{USAGE}"),
    }
}

fn list() -> anyhow::Result<()> {
    for game in arkor_stores::library_games()? {
        println!("{:<40} {}", game.id.to_string(), game.name);
    }
    Ok(())
}

fn detect() -> anyhow::Result<()> {
    let library = Library::load()?;
    let (found, errors) = arkor_stores::detect();
    for d in found {
        let mark = if library.has_origin(&d.game.id.to_string()) {
            "in library"
        } else {
            ""
        };
        let how = match &d.new {
            Ok(_) => String::new(),
            Err(e) => format!("  (cannot add: {e})"),
        };
        println!(
            "{:<44} {:<36} {mark}{how}",
            d.game.id.to_string(),
            d.game.name
        );
    }
    for e in errors {
        eprintln!("warning: {e:#}");
    }
    Ok(())
}

fn added(new: NewGame) -> anyhow::Result<()> {
    let mut library = Library::load()?;
    let id = library.add(new);
    library.save()?;
    println!("added local:{id}");
    Ok(())
}

fn add(args: &[String]) -> anyhow::Result<()> {
    let exe = PathBuf::from(arg(args, 0)?);
    anyhow::ensure!(exe.exists(), "{} not found", exe.display());
    let mut new = NewGame::windows(arkor_stores::prefix::suggested_name(&exe), exe);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                new.name = arg(args, i + 1)?.clone();
                i += 1;
            }
            "--prefix" => {
                let p = PathBuf::from(arg(args, i + 1)?);
                new.prefix = Some(arkor_stores::prefix::resolve(&p).unwrap_or(p));
                i += 1;
            }
            "--linux" => new.kind = GameKind::Linux,
            "--" => {
                new.args = args[i + 1..].to_vec();
                break;
            }
            other => anyhow::bail!("unknown option `{other}`"),
        }
        i += 1;
    }
    added(new)
}

fn add_detected(id: &str) -> anyhow::Result<()> {
    let id: GameId = id.parse()?;
    let (found, _) = arkor_stores::detect();
    let d = found
        .into_iter()
        .find(|d| d.game.id == id)
        .with_context(|| format!("`{id}` not found; see `arkor detect`"))?;
    added(d.new.map_err(|e| anyhow!(e))?)
}

fn import(prefix_arg: &str, exe: Option<&String>) -> anyhow::Result<()> {
    use arkor_stores::prefix;
    let root = prefix::resolve(std::path::Path::new(prefix_arg))
        .ok_or_else(|| anyhow!("not a Wine prefix (no drive_c folder found): {prefix_arg}"))?;
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
    let exe = PathBuf::from(exe);
    anyhow::ensure!(exe.is_file(), "{} not found", exe.display());
    let mut new = NewGame::windows(prefix::suggested_name(&exe), exe);
    new.prefix = Some(root);
    added(new)
}

fn tool(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::start::{PrefixTool, run_prefix_tool};
    let id: GameId = arg(args, 0)?.parse()?;
    let tool = match arg(args, 1)?.as_str() {
        "winecfg" => PrefixTool::Winecfg,
        "winetricks" => PrefixTool::Winetricks(args[2..].to_vec()),
        "run" => PrefixTool::Run(PathBuf::from(arg(args, 2)?)),
        other => anyhow::bail!("unknown tool `{other}`\n{USAGE}"),
    };
    let status = run_prefix_tool(&id, &tool)?;
    std::process::exit(status.code().unwrap_or(1));
}

fn proton(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::proton_dl::{self, Variant};
    let variant = |s: &str| -> anyhow::Result<Variant> {
        Ok(match s {
            "ge" => Variant::Ge,
            "cachyos" => Variant::CachyOs,
            "dw" => Variant::Dw,
            "wineland" => Variant::Wineland,
            other => anyhow::bail!("unknown variant `{other}` (ge, cachyos, dw, wineland)"),
        })
    };
    match arg(args, 0)?.as_str() {
        "installed" => {
            for (name, path) in arkor_launch::tools::installed_protons() {
                println!("{name:<36} {}", path.display());
            }
        }
        "list" => {
            for r in proton_dl::list(variant(arg(args, 1)?)?)? {
                println!(
                    "{:<36} {:>8}  {}",
                    r.tag,
                    arkor_stores::prefix::human_size(r.size),
                    r.url
                );
            }
        }
        "install" => {
            let tag = arg(args, 2)?;
            let release = proton_dl::list(variant(arg(args, 1)?)?)?
                .into_iter()
                .find(|r| &r.tag == tag)
                .with_context(|| format!("release `{tag}` not found"))?;
            println!("installed {}", proton_dl::install(&release)?.display());
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}

fn optiscaler(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::optiscaler;
    match args.first().map(String::as_str) {
        Some("download") => {
            let channel = if args.iter().any(|a| a == "--edge") {
                optiscaler::Channel::Edge
            } else {
                optiscaler::Channel::Stable
            };
            let release = optiscaler::latest_release_in(channel)?;
            println!(
                "downloading OptiScaler {} ({} MB)…",
                release.tag,
                release.size / 1_000_000
            );
            println!("ready: {}", optiscaler::download(&release)?.display());
        }
        Some("fsr4") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let what = arg(args, 2)?;
            if what == "remove" {
                optiscaler::uninstall_mod(optiscaler::Mod::Fsr4, &id)?;
                println!("custom FSR4 removed");
            } else {
                optiscaler::install_fsr4(&id, Path::new(what))?;
                println!("custom FSR4 installed");
            }
        }
        Some("optipatcher") => {
            let id: GameId = arg(args, 1)?.parse()?;
            if args.get(2).is_some_and(|a| a == "remove") {
                optiscaler::remove_optipatcher(&id)?;
                println!("OptiPatcher removed");
            } else {
                optiscaler::install_optipatcher(&id)?;
                println!("OptiPatcher installed; ASI plugins enabled");
            }
        }
        Some("install") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let dir = PathBuf::from(arg(args, 2)?);
            let proxy = args.get(3).map_or("dxgi", String::as_str);
            let (tag, source) = optiscaler::downloaded().into_iter().next().ok_or_else(|| {
                anyhow!("no OptiScaler downloaded yet: run `arkor optiscaler download`")
            })?;
            let m = optiscaler::install(&id, &tag, &source, &dir, proxy)?;
            println!(
                "installed OptiScaler {tag} as {} ({} files, {} game files backed up)",
                m.dll,
                m.created.len(),
                m.backed_up.len()
            );
        }
        Some("remove") => {
            optiscaler::uninstall(&arg(args, 1)?.parse()?)?;
            println!("removed");
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}

fn dlss_enabler(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::dlss_enabler as de;
    match args.first().map(String::as_str) {
        Some("download") => {
            let build = de::builds()?.into_iter().next().context("no build found")?;
            println!("downloading DLSS Enabler {}…", build.version);
            println!("ready: {}", de::download(&build)?.display());
        }
        Some("install") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let (version, source) = de::downloaded()
                .into_iter()
                .next()
                .context("run `arkor dlss-enabler download` first")?;
            let m = de::install(&id, &version, &source, &PathBuf::from(arg(args, 2)?))?;
            println!(
                "installed DLSS Enabler {version} as {} ({} game files backed up)",
                m.dll,
                m.backed_up.len()
            );
        }
        Some("remove") => {
            de::uninstall(&arg(args, 1)?.parse()?)?;
            println!("removed");
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}

fn streamline(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::streamline;
    match args.first().map(String::as_str) {
        Some("download") => {
            let v = streamline::latest_version()?;
            println!("downloading Streamline SDK {v} (about 280 MB)…");
            println!("ready: {}", streamline::download(&v)?.display());
        }
        Some("install") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let (v, sdk) = streamline::downloaded()
                .into_iter()
                .next()
                .context("run `arkor streamline download` first")?;
            let m = streamline::install(&id, &v, &sdk, Path::new(arg(args, 2)?))?;
            for f in &m.created {
                println!("updated {}", f.display());
            }
        }
        Some("remove") => {
            streamline::uninstall(&arg(args, 1)?.parse()?)?;
            println!("restored the game's own DLLs");
        }
        _ => anyhow::bail!("usage: arkor streamline download | install <id> <dir> | remove <id>"),
    }
    Ok(())
}

fn shortcut(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::shortcuts;
    let id: GameId = arg(args, 0)?.parse()?;
    let name = arkor_stores::library_games()?
        .into_iter()
        .find(|g| g.id == id)
        .map_or_else(|| id.id.clone(), |g| g.name);
    let icon = arkor_core::library::Library::load()
        .ok()
        .and_then(|l| l.games.into_iter().find(|g| g.id == id.id))
        .and_then(|g| arkor_launch::covers::cover_for(&g));
    match arg(args, 1)?.as_str() {
        "menu" => println!(
            "{}",
            shortcuts::add_menu_entry(&id, &name, icon.as_deref(), false)?.display()
        ),
        "desktop" => println!(
            "{}",
            shortcuts::add_menu_entry(&id, &name, icon.as_deref(), true)?.display()
        ),
        "steam" => println!(
            "added to {} Steam account(s); start Steam to see it",
            shortcuts::add_to_steam(&id, &name, icon.as_deref())?
        ),
        "remove" => shortcuts::remove_menu_entry(&id),
        "command" => println!("{}", shortcuts::launch_command(&id)),
        other => anyhow::bail!("unknown shortcut kind {other}"),
    }
    Ok(())
}

fn bundle(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::bundles::{self, Bundle};
    let kind = |s: &str| -> anyhow::Result<Bundle> {
        Ok(match s {
            "helixsr" => Bundle::HelixSr,
            "dlssnr-rdna4" => Bundle::DlssNrRdna4,
            "dlssnr-rdna3" => Bundle::DlssNrRdna3,
            other => anyhow::bail!("unknown bundle {other}"),
        })
    };
    let ready = |b: Bundle| {
        bundles::downloaded(b)
            .into_iter()
            .next()
            .with_context(|| format!("run `arkor bundle download` for {} first", b.name()))
    };
    match args.first().map(String::as_str) {
        Some("download") => {
            let b = kind(arg(args, 1)?)?;
            println!("downloading {} (a few hundred MB)…", b.name());
            let (v, dir) = bundles::download(b)?;
            println!("{} {v} ready: {}", b.name(), dir.display());
        }
        Some("helixsr") => {
            let (_, dir) = ready(Bundle::HelixSr)?;
            let game = Path::new(arg(args, 1)?);
            if args.get(2).is_some_and(|a| a == "remove") {
                bundles::helixsr_remove(&dir, game)?;
            } else {
                bundles::helixsr_setup(&dir, game)?;
            }
            println!("opened in a terminal");
        }
        Some("dlssnr") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let b = [Bundle::DlssNrRdna4, Bundle::DlssNrRdna3]
                .into_iter()
                .find(|b| !bundles::downloaded(*b).is_empty())
                .context("run `arkor bundle download dlssnr-rdna4` (or -rdna3) first")?;
            let (v, dir) = ready(b)?;
            let model = args.get(4).map_or("CC_SMALL_FP8", String::as_str);
            let out = bundles::dlssnr_install(
                &id,
                b,
                &v,
                &dir,
                Path::new(arg(args, 2)?),
                Path::new(arg(args, 3)?),
                model,
            )?;
            println!("{out}");
        }
        Some("dlssnr-remove") => {
            bundles::dlssnr_remove(&arg(args, 1)?.parse()?)?;
            println!("removed");
        }
        _ => anyhow::bail!("usage: see `arkor help`"),
    }
    Ok(())
}

fn self_update(check_only: bool) -> anyhow::Result<()> {
    use arkor_launch::self_update;
    match self_update::check()? {
        None => println!("Arkor {} is up to date", self_update::current_version()),
        Some(r) if check_only => println!(
            "Arkor {} is out (you have {})",
            r.version,
            self_update::current_version()
        ),
        Some(r) => {
            println!("updating to Arkor {}…", r.version);
            let exe = self_update::install(&r)?;
            println!("installed: {}", exe.display());
        }
    }
    Ok(())
}

fn layer(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::layers;
    let tool = |ix: usize| -> anyhow::Result<&'static layers::LayerTool> {
        let id = arg(args, ix)?;
        layers::TOOLS
            .iter()
            .find(|t| t.id == id)
            .with_context(|| format!("unknown layer {id}"))
    };
    match args.first().map(String::as_str) {
        Some("list") | None => {
            for t in &layers::TOOLS {
                println!(
                    "{:8} {:6} {}",
                    t.id,
                    if layers::arkor_installed(t.id) {
                        "arkor"
                    } else {
                        "-"
                    },
                    t.description_en
                );
            }
            println!("\nimplicit layers switched on by a variable:");
            for l in layers::system_layers() {
                println!("  {}={}  {}", l.enable.0, l.enable.1, l.name);
            }
        }
        Some("install") => {
            let t = tool(1)?;
            println!("installed {} {}", t.name, layers::install(t)?);
        }
        Some("remove") => {
            layers::uninstall(tool(1)?.id)?;
            println!("removed");
        }
        Some(other) => anyhow::bail!("unknown layer command {other}"),
    }
    Ok(())
}

fn reshade(args: &[String]) -> anyhow::Result<()> {
    use arkor_launch::reshade;
    match args.first().map(String::as_str) {
        Some("download") => {
            let v = reshade::latest_version()?;
            println!("downloading ReShade {v} and its shaders…");
            println!("ready: {}", reshade::download(&v)?.display());
        }
        Some("install") => {
            let id: GameId = arg(args, 1)?.parse()?;
            let dir = PathBuf::from(arg(args, 2)?);
            let proxy = args
                .get(3)
                .filter(|a| !a.starts_with("--"))
                .map_or("dxgi", String::as_str);
            let bits32 = args.iter().any(|a| a == "--32");
            let (v, source) = reshade::downloaded()
                .into_iter()
                .next()
                .context("run `arkor reshade download` first")?;
            let m = reshade::install(&id, &v, &source, &dir, proxy, bits32)?;
            println!(
                "installed ReShade {v} as {} ({} items, {} game files backed up)",
                m.dll,
                m.created.len(),
                m.backed_up.len()
            );
        }
        Some("remove") => {
            reshade::uninstall(&arg(args, 1)?.parse()?)?;
            println!("removed");
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}

/// Prefixes Arkor created (inside its prefixes folder) for library games.
fn own_prefixes() -> anyhow::Result<Vec<PathBuf>> {
    let global = arkor_core::Global::load_or_import()?;
    let dir = global.prefixes_dir();
    Ok(Library::load()?
        .games
        .iter()
        .filter(|g| g.needs_prefix())
        .map(|g| g.effective_prefix(&dir))
        .filter(|p| p.starts_with(&dir) && p.exists())
        .collect())
}

fn backup(args: &[String]) -> anyhow::Result<()> {
    use arkor_core::backup;
    match args.first().map(String::as_str) {
        Some("create") => {
            let global = arkor_core::Global::load_or_import()?;
            let prefixes = if args.iter().any(|a| a == "--with-prefixes") {
                own_prefixes()?
            } else {
                Vec::new()
            };
            let (file, skipped) = backup::create(&global.backup.dest_dir(), &prefixes, "manual")?;
            println!("backup written: {}", file.display());
            for p in skipped {
                eprintln!("skipped (outside home): {}", p.display());
            }
        }
        Some("restore") => {
            backup::restore(std::path::Path::new(arg(args, 1)?))?;
            println!("restored");
        }
        _ => anyhow::bail!(USAGE),
    }
    Ok(())
}
