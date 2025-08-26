use color_eyre::Result;
use ec_demo::app::App;

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();

    #[cfg(not(feature = "mock"))]
    let source = ec_demo::acpi::Acpi::default();

    #[cfg(feature = "mock")]
    let source = ec_demo::mock::Mock::default();

    // TODO: Use clap in the future if more args are expected
    // This just uses the first arg as the elf path
    let elf_path = std::env::args().nth(1).map(std::path::PathBuf::from);

    App::new(source, elf_path)?.run(terminal)
}
