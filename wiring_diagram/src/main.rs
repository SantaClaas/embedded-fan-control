//! Draws the wiring diagrams in `fan-controller/documentation/`.
//!
//!     cd wiring_diagram && cargo run
//!
//! The SVGs it writes are checked in, so run it and commit the result. Nothing
//! else in the workspace depends on this crate; it exists so that moving a wire
//! is an edit to a coordinate rather than to an SVG path.

mod sheets;
mod svg;

use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    // Relative to this crate rather than to the working directory, so the
    // drawings land in the same place wherever cargo is invoked from.
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fan-controller")
        .join("documentation");

    for (name, sheet) in [
        ("wiring-overview", sheets::overview()),
        ("wiring-fans", sheets::fans()),
        ("wiring-relay", sheets::relay()),
        ("wiring-button-leds", sheets::panel()),
    ] {
        let path = out.join(format!("{name}.svg"));
        std::fs::write(&path, sheet.render())?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
