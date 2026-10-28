//! Only compiled for the explicit Windows launch test fixture feature.
use super::*;
pub fn run()->io::Result<()> {
    // Refuse direct owner execution before any fixture filesystem access.
    let parent=verify_process(unsafe{GetCurrentProcess()})?;
    let args:Vec<String>=std::env::args().skip(1).collect();
    if args==["--exit-probe"] { return Ok(()); }
    if args==["--child-probe"] {
        io::stdout().write_all(b"fixture-child-lpac")?;
        return Ok(());
    }
    if args.len()!=3 || !["--launch-probe","--launch-no-stdio-probe"].contains(&args[0].as_str()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"Closed fixture arguments required"));
    }
    let no_stdio=args[0]=="--launch-no-stdio-probe";
    let executable=PathBuf::from(&args[1]);let workspace=PathBuf::from(&args[2]);
    kind(&executable,false)?;kind(&workspace,true)?;
    let log=OpenOptions::new().read(true).write(true).create_new(true).open(workspace.join("launch.log"))?;
    let system=std::env::var_os("SystemRoot").ok_or_else(||denied("Missing Host SystemRoot"))?;
    let mut command=Command::new(executable);
    command.arg(if no_stdio{"--exit-probe"}else{"--child-probe"}).current_dir(&workspace).env_clear()
        .env("SystemRoot",&system).env("WINDIR",&system)
        .env("TEMP",&workspace).env("TMP",&workspace);
    let mut phase="fixture_start";
    let result=(||->io::Result<()>{
        let (mut child,input,mut output)=if no_stdio{
            native_child::spawn_without_stdio_for_test(&command,&log,&parent.container,&mut phase)?
        }else{native_child::spawn(&command,&log,&parent.container,&mut phase)?};
        phase="fixture_child_token";
        if verify_process(child.as_raw_handle())?!=parent{return Err(denied("Fixture child identity differs"));}
        phase="fixture_child_job";native_child::verify_same_job(&child)?;
        phase="fixture_resume";child.resume()?;drop(input);
        if !no_stdio {
            let mut bytes=Vec::new();Read::by_ref(&mut output).take(8193).read_to_end(&mut bytes)?;
            if bytes!=b"fixture-child-lpac"{return Err(io::Error::other("Unexpected fixture child bytes"));}
        }
        let exit=child.wait()?;
        if !exit.success(){return Err(io::Error::other("Fixture child did not exit successfully"));}
        Ok(())
    })();
    let mut text=String::new();let mut reader=log.try_clone()?;reader.seek(SeekFrom::Start(0))?;
    reader.take(8192).read_to_string(&mut text)?;
    let row=serde_json::json!({"fixture":true,"native_blender":false,"ok":result.is_ok(),
        "phase":phase,"os_error":result.as_ref().err().and_then(io::Error::raw_os_error),
        "diagnostics":text});
    println!("{row}");
    result
}
