use crate::commands::project;
use crate::config::Config;
use crate::engine::{EngineRelease, list_engine_releases};
use crate::package_manager::PackageManager;
use crate::project::validate_project_name;
use crate::ui::{self, Tone};
use anyhow::{Context, Result, bail};
use dialoguer::{Input, Select, theme::ColorfulTheme};
use std::io::IsTerminal;

pub fn create(verbose: bool) -> Result<i32> {
    require_interactive(std::io::stdin().is_terminal() && std::io::stderr().is_terminal())?;

    let config = Config::load_default()?;
    let releases = list_engine_releases().context("could not load BornEngine versions from npm")?;
    let mut prompts = TerminalPrompts::default();

    println!("{}", ui::paint("Create a BornEngine game", Tone::Heading));
    let choices = collect_choices(&config, &releases, &mut prompts)?;
    project::new(
        &choices.project_name,
        Some(choices.package_manager),
        Some(choices.engine_release.version),
        None,
        verbose,
    )
}

fn require_interactive(available: bool) -> Result<()> {
    if !available {
        bail!(
            "`bornengine create` requires an interactive terminal; use `bornengine new <name> --package-manager <pnpm|npm|yarn> --engine-version <version>` for non-interactive use"
        );
    }
    Ok(())
}

#[derive(Debug)]
struct CreateChoices {
    project_name: String,
    package_manager: PackageManager,
    engine_release: EngineRelease,
}

trait CreatePrompts {
    fn input_text(&mut self, prompt: &str) -> Result<String>;
    fn select(&mut self, prompt: &str, options: &[String], default: usize) -> Result<usize>;
}

#[derive(Default)]
struct TerminalPrompts {
    theme: ColorfulTheme,
}

impl CreatePrompts for TerminalPrompts {
    fn input_text(&mut self, prompt: &str) -> Result<String> {
        Input::<String>::with_theme(&self.theme)
            .with_prompt(prompt)
            .validate_with(|name: &String| {
                validate_project_name(name).map_err(|error| error.to_string())
            })
            .interact_text()
            .context("could not read project name")
    }

    fn select(&mut self, prompt: &str, options: &[String], default: usize) -> Result<usize> {
        Select::with_theme(&self.theme)
            .with_prompt(prompt)
            .items(options)
            .default(default)
            .max_length(12)
            .interact()
            .context("could not read selection")
    }
}

fn collect_choices(
    config: &Config,
    releases: &[EngineRelease],
    prompts: &mut impl CreatePrompts,
) -> Result<CreateChoices> {
    if releases.is_empty() {
        bail!("no stable BornEngine releases are available from npm");
    }

    let project_name = prompts.input_text("Project name")?;
    validate_project_name(&project_name)?;

    let managers = [
        PackageManager::Pnpm,
        PackageManager::Npm,
        PackageManager::Yarn,
    ];
    let manager_options = managers
        .iter()
        .map(|manager| manager.as_str().to_owned())
        .collect::<Vec<_>>();
    let configured_manager = PackageManager::parse(&config.package_manager)?;
    let manager_default = managers
        .iter()
        .position(|manager| *manager == configured_manager)
        .unwrap_or(0);
    let manager_index = prompts.select("Package manager", &manager_options, manager_default)?;
    let package_manager = managers
        .get(manager_index)
        .copied()
        .context("package-manager selection is outside the available options")?;

    let version_options = releases
        .iter()
        .enumerate()
        .map(|(index, release)| {
            if index == 0 {
                format!("{} ({}) — latest", release.version, release.package_name)
            } else {
                format!("{} ({})", release.version, release.package_name)
            }
        })
        .collect::<Vec<_>>();
    let version_default = releases
        .iter()
        .position(|release| release.version == config.engine_version)
        .unwrap_or(0);
    let version_index = prompts.select("BornEngine version", &version_options, version_default)?;
    let engine_release = releases
        .get(version_index)
        .cloned()
        .context("BornEngine version selection is outside the available options")?;

    Ok(CreateChoices {
        project_name,
        package_manager,
        engine_release,
    })
}

#[cfg(test)]
mod tests {
    use super::{CreatePrompts, collect_choices, require_interactive};
    use crate::config::Config;
    use crate::engine::EngineRelease;
    use crate::package_manager::PackageManager;
    use anyhow::Result;

    #[derive(Debug)]
    struct SelectionCall {
        prompt: String,
        options: Vec<String>,
        default: usize,
    }

    #[derive(Debug)]
    struct FakePrompts {
        name: String,
        selections: Vec<usize>,
        calls: Vec<SelectionCall>,
    }

    impl CreatePrompts for FakePrompts {
        fn input_text(&mut self, _prompt: &str) -> Result<String> {
            Ok(self.name.clone())
        }

        fn select(&mut self, prompt: &str, options: &[String], default: usize) -> Result<usize> {
            self.calls.push(SelectionCall {
                prompt: prompt.to_owned(),
                options: options.to_vec(),
                default,
            });
            Ok(self.selections.remove(0))
        }
    }

    fn releases() -> Vec<EngineRelease> {
        vec![
            EngineRelease {
                package_name: "@bornengine/engine".to_owned(),
                version: "0.4.16".to_owned(),
            },
            EngineRelease {
                package_name: "@bornengine/engine".to_owned(),
                version: "0.4.15".to_owned(),
            },
        ]
    }

    #[test]
    fn wizard_uses_configured_defaults_and_returns_the_selected_release() {
        let config = Config {
            package_manager: "npm".to_owned(),
            engine_version: "0.4.15".to_owned(),
        };
        let releases = releases();
        let mut prompts = FakePrompts {
            name: "MyGame".to_owned(),
            selections: vec![2, 0],
            calls: Vec::new(),
        };

        let choices = collect_choices(&config, &releases, &mut prompts).unwrap();

        assert_eq!(choices.project_name, "MyGame");
        assert_eq!(choices.package_manager, PackageManager::Yarn);
        assert_eq!(choices.engine_release, releases[0]);
        assert_eq!(prompts.calls[0].prompt, "Package manager");
        assert_eq!(prompts.calls[0].options, ["pnpm", "npm", "yarn"]);
        assert_eq!(prompts.calls[0].default, 1);
        assert_eq!(prompts.calls[1].prompt, "BornEngine version");
        assert!(prompts.calls[1].options[0].contains("0.4.16"));
        assert!(prompts.calls[1].options[1].contains("0.4.15"));
        assert_eq!(prompts.calls[1].default, 1);
    }

    #[test]
    fn unavailable_configured_version_defaults_to_the_first_published_release() {
        let config = Config {
            package_manager: "pnpm".to_owned(),
            engine_version: "0.1.0".to_owned(),
        };
        let mut prompts = FakePrompts {
            name: "MyGame".to_owned(),
            selections: vec![0, 0],
            calls: Vec::new(),
        };

        collect_choices(&config, &releases(), &mut prompts).unwrap();

        assert_eq!(prompts.calls[0].default, 0);
        assert_eq!(prompts.calls[1].default, 0);
    }

    #[test]
    fn invalid_project_name_is_rejected_before_showing_selectors() {
        let mut prompts = FakePrompts {
            name: "../escape".to_owned(),
            selections: Vec::new(),
            calls: Vec::new(),
        };

        let error = collect_choices(&Config::default(), &releases(), &mut prompts).unwrap_err();

        assert!(error.to_string().contains("single directory name"));
        assert!(prompts.calls.is_empty());
    }

    #[test]
    fn empty_release_list_fails_before_prompting() {
        let mut prompts = FakePrompts {
            name: "MyGame".to_owned(),
            selections: Vec::new(),
            calls: Vec::new(),
        };

        let error = collect_choices(&Config::default(), &[], &mut prompts).unwrap_err();

        assert!(error.to_string().contains("stable BornEngine releases"));
        assert!(prompts.calls.is_empty());
    }

    #[test]
    fn create_requires_a_terminal_and_points_to_the_noninteractive_command() {
        let error = require_interactive(false).unwrap_err();

        assert!(error.to_string().contains("bornengine new"));
    }
}
