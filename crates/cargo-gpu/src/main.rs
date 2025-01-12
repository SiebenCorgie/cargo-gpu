//! Rust GPU shader crate builder.
//!
//! This program manages installations of `spirv-builder-cli` and `rustc_codegen_spirv`.
//! It uses these tools to compile Rust code into SPIR-V.
//!
//! # How it works
//!
//! In order to build shader crates, we must invoke cargo/rustc with a special backend
//! that performs the SPIR-V code generation. This backend is a dynamic library known
//! by its project name `rustc_codegen_spirv`. The name of the artifact itself is
//! OS-dependent.
//!
//! There are a lot of special flags to wrangle and so we use a command line program
//! that wraps `cargo` to perform the building of shader crates. This cli program is
//! called `spirv-builder-cli`, which itself is a cli wrapper around the `spirv-builder`
//! library.
//!
//! ## Where the binaries are
//!
//! `cargo-gpu` maintains different versions `spirv-builder-cli` and `rustc_codegen_spirv`
//! in a cache dir. The location is OS-dependent, for example on macOS it's in
//! `~/Library/Caches/rust-gpu`. Specific versions live inside the cache dir, prefixed
//! by their `spirv-builder` cargo dependency and rust toolchain pair.
//!
//! Building a specific "binary pair" of `spirv-builder-cli` and `rustc_codegen_spirv`
//! happens when there is no existing pair that matches the computed prefix, or if
//! a force rebuild is specified on the command line.
//!
//! ## Building the "binary pairs"
//!
//! The source of `spirv-builder-cli` lives alongside this source file, in crate that
//! is not included in the workspace. That same source code is also included statically
//! in **this** source file.
//!
//! When `spirv-builder-cli` needs to be built, a new directory is created in the cache
//! where the source to `spirv-builder-cli` is copied into, containing the specific cargo
//! dependency for `spirv-builder` and the matching rust toolchain channel.
//!
//! Then `cargo` is invoked in that cache directory to build the pair of artifacts, which
//! are then put into the top level of that cache directory.
//!
//! This pair of artifacts is then used to build shader crates.
//!
//! ## Building shader crates
//!
//! `cargo-gpu` takes a path to a shader crate to build, as well as a path to a directory
//! to put the compiled `spv` source files. It also takes a path to an output mainifest
//! file where all shader entry points will be mapped to their `spv` source files. This
//! manifest file can be used by build scripts (`build.rs` files) to generate linkage or
//! conduct other post-processing, like converting the `spv` files into `wgsl` files,
//! for example.

use anyhow::Context as _;

use build::Build;
use clap::Parser as _;
use install::Install;
use show::Show;
use toml::Toml;

mod build;
mod install;
mod show;
mod spirv_cli;
mod spirv_source;
mod toml;

/// Central function to write to the user.
#[macro_export]
macro_rules! user_output {
    ($($args: tt)*) => {
        #[allow(
            clippy::allow_attributes,
            clippy::useless_attribute,
            unused_imports,
            reason = "`std::io::Write` is only sometimes called??"
        )]
        use std::io::Write as _;

        #[expect(
            clippy::non_ascii_literal,
            reason = "CRAB GOOD. CRAB IMPORTANT."
        )]
        {
            print!("🦀 ");
        }
        print!($($args)*);
        std::io::stdout().flush().unwrap();
   }
}

fn main() {
    #[cfg(debug_assertions)]
    std::env::set_var("RUST_BACKTRACE", "1");

    env_logger::builder().init();

    if let Err(error) = run() {
        log::error!("{error:?}");

        #[expect(
            clippy::print_stderr,
            reason = "Our central place for outputting error messages"
        )]
        {
            eprintln!("Error: {error}");
        }
    };
}

/// Wrappable "main" to catch errors.
fn run() -> anyhow::Result<()> {
    let args = std::env::args()
        .filter(|arg| {
            // Calling cargo-gpu as the cargo subcommand "cargo gpu" passes "gpu"
            // as the first parameter, which we want to ignore.
            arg != "gpu"
        })
        .collect::<Vec<_>>();
    log::trace!("args: {args:?}");
    let cli = Cli::parse_from(args);

    match cli.command {
        Command::Install(install) => {
            log::debug!("installing with arguments: {install:#?}");
            let (_, _) = install.run()?;
        }
        Command::Build(mut build) => {
            log::debug!("building with arguments: {build:#?}");
            build.run()?;
        }
        Command::Toml(toml) => {
            log::debug!("building by toml file with arguments: {toml:#?}");
            toml.run()?;
        }
        Command::Show(show) => show.run()?,
        Command::DumpUsage => dump_full_usage_for_readme()?,
    };

    Ok(())
}

/// All of the available subcommands for `cargo gpu`
#[derive(clap::Subcommand)]
enum Command {
    /// Install rust-gpu compiler artifacts.
    Install(Install),

    /// Compile a shader crate to SPIR-V.
    Build(Build),

    /// Compile a shader crate according to the `cargo gpu build` parameters
    /// found in the given toml file.
    Toml(Toml),

    /// Show some useful values.
    Show(Show),

    /// A hidden command that can be used to recursively print out all the subcommand help messages:
    ///   `cargo gpu dump-usage`
    /// Useful for updating the README.
    #[clap(hide(true))]
    DumpUsage,
}

#[derive(clap::Parser)]
#[clap(author, version, about, subcommand_required = true)]
pub(crate) struct Cli {
    /// The command to run.
    #[clap(subcommand)]
    command: Command,
}

fn cache_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = directories::BaseDirs::new()
        .with_context(|| "could not find the user home directory")?
        .cache_dir()
        .join("rust-gpu");

    Ok(if cfg!(test) {
        let thread_id = std::thread::current().id();
        let id = format!("{thread_id:?}").replace('(', "-").replace(')', "");
        dir.join("tests").join(id)
    } else {
        dir
    })
}

/// Location of the target spec metadata files
fn target_spec_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = cache_dir()?.join("target-specs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Convenience function for internal use. Dumps all the CLI usage instructions. Useful for
/// updating the README.
fn dump_full_usage_for_readme() -> anyhow::Result<()> {
    use clap::CommandFactory as _;
    let mut command = Cli::command();

    let mut buffer: Vec<u8> = Vec::default();
    command.build();

    write_help(&mut buffer, &mut command, 0)?;
    user_output!("{}", String::from_utf8(buffer)?);

    Ok(())
}

/// Recursive function to print the usage instructions for each subcommand.
fn write_help(
    buffer: &mut impl std::io::Write,
    cmd: &mut clap::Command,
    _depth: usize,
) -> anyhow::Result<()> {
    if cmd.get_name() == "help" {
        return Ok(());
    }

    let mut command = cmd.get_name().to_owned();
    writeln!(
        buffer,
        "\n* {}{}",
        command.remove(0).to_uppercase(),
        command
    )?;
    writeln!(buffer)?;
    cmd.write_long_help(buffer)?;

    for sub in cmd.get_subcommands_mut() {
        writeln!(buffer)?;
        #[expect(clippy::used_underscore_binding, reason = "Used in recursion only")]
        write_help(buffer, sub, _depth + 1)?;
    }

    Ok(())
}

/// Returns a string suitable to use as a directory.
///
/// Created from the spirv-builder source dep and the rustc channel.
fn to_dirname(text: &str) -> String {
    text.replace(
        [std::path::MAIN_SEPARATOR, '\\', '/', '.', ':', '@', '='],
        "_",
    )
    .split(['{', '}', ' ', '\n', '"', '\''])
    .collect::<Vec<_>>()
    .concat()
}

#[cfg(test)]
mod test {
    use crate::cache_dir;

    pub fn shader_crate_template_path() -> std::path::PathBuf {
        let project_base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        project_base.join("../shader-crate-template")
    }

    pub fn tests_teardown() {
        let cache_dir = cache_dir().unwrap();
        if !cache_dir.exists() {
            return;
        }
        std::fs::remove_dir_all(cache_dir).unwrap();
    }
}
