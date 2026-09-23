use semwright_driver_motion_canvas::{compiler, validate};
use std::{env, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or("semantic path missing")?);
    let output = PathBuf::from(args.next().ok_or("output path missing")?);
    if args.next().is_some() {
        return Err("too many arguments".into());
    }
    let bytes = fs::read(source)?;
    let project = validate::parse(&bytes)?;
    let generated = compiler::compile(&project)?;
    if output.exists() {
        fs::remove_dir_all(&output)?;
    }
    fs::create_dir(&output)?;
    for (relative, data) in generated.files {
        let path = output.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, data)?;
    }
    println!("{}", output.display());
    Ok(())
}
