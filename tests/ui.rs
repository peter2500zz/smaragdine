//! Compile the private macro as downstream callers would. Snapshots protect
//! messages and source locations, including errors referring to our Git fork.
//! Run `cargo test --test ui -- --bless` (also with `--features async`) to
//! intentionally update snapshots after reviewing a diagnostic change.

use std::path::Path;
use ui_test::{Config, dependencies::DependencyBuilder, spanned::Spanned};

fn main() -> ui_test::Result<()> {
    let mut config = Config::rustc("tests/ui");
    config
        .skip_files
        .extend(["support.rs".into(), "/dependencies/".into()]);
    // The full stderr snapshot checks the diagnostic and its location. Several
    // macro errors deliberately originate in a different source file.
    config.comment_defaults.base().require_annotations = Spanned::dummy(false).into();
    config.comment_defaults.base().add_custom("no-rustfix", ());

    let mut dependencies = DependencyBuilder {
        crate_manifest_path: Path::new("tests/ui/dependencies/Cargo.toml").into(),
        ..DependencyBuilder::default()
    };
    dependencies.program.args.push("--lib".into());
    dependencies.program.args.push("--locked".into());

    #[cfg(feature = "async")]
    {
        // Match a directory boundary: `sync_fail/` is also a substring of
        // `async_fail/`, whose negative tests must run in this configuration.
        config.skip_files.push("/sync_fail/".into());
        config
            .program
            .args
            .extend(["--cfg".into(), "feature=\"async\"".into()]);
        dependencies
            .program
            .args
            .extend(["--features".into(), "async".into()]);
    }
    #[cfg(not(feature = "async"))]
    config
        .skip_files
        .extend(["/async_fail/".into(), "/async_pass/".into()]);

    config
        .comment_defaults
        .base()
        .add_custom("dependencies", dependencies);
    config.path_filter(Path::new(env!("CARGO_MANIFEST_DIR")), "$DIR");
    // Git checkouts include a user-specific Cargo home and revision directory.
    // Keep the referenced source file, line, message and all user spans intact.
    config.stderr_filter(
        r"(?m)((?:-->|:::) )[^\n]*[/\\]azalea-brigadier[/\\]",
        "$1$$AZALEA_BRIGADIER/",
    );
    config.stderr_filter(
        r"(?m)((?:-->|:::) )[^\n]*[/\\]rustlib/src/rust/library/",
        "$1$$RUST/",
    );
    config.stderr_filter(r"(?m)((?:-->|:::) )/rustc/[0-9a-f]+/library/", "$1$$RUST/");
    config.stderr_filter(r"\n+\z", "\n");
    ui_test::run_tests(config)
}
