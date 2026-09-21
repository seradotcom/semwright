use clap::{Parser,CommandFactory};
use semwright_cli::*;
use semwright_protocol::{self as ipc,ClientMessage,ServerMessage};
use semwright_types::*;
use serde_json::json;
use std::{path::PathBuf,time::Duration};
async fn local(cli:&Cli)->Result<bool>{
    match &cli.command{
        Command::Completions{shell}=>{clap_complete::generate(*shell,&mut Cli::command(),"computerctl",&mut std::io::stdout());},
        Command::Man=>{clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;},
        Command::Config{..}=>{let runtime=ipc::runtime_directory()?;let home=std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();let config=std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(||home.join(".config"));let state=std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(||home.join(".local/state"));print_result(&json!({"runtime":runtime,"socket":cli.socket.clone().unwrap_or_else(||runtime.join("broker.sock")),"config":config.join("semwright/daemon.toml"),"state":state.join("semwright")}),cli.json)?;},
        Command::Recipe{command:Recipe::Scaffold{name,output}}=>{
            if name.is_empty()||name.len()>64||!name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'){return Err(Error::invalid("Recipe name must be lowercase letters, digits or hyphens"));}
            let template=include_str!("../../../recipes/fake-export.yaml").replacen("name: fake-export",&format!("name: {name}"),1);create(output,&template)?;print_result(&json!({"created":output,"installed":false}),cli.json)?;
        },
        Command::Plugin{command:Plugin::Scaffold{output,sdk_path}}=>{
            let sdk=sdk_path.canonicalize()?;if !sdk.join("Cargo.toml").is_file(){return Err(Error::invalid("--sdk-path must be the semwright-plugin-sdk crate directory"));}
            let parent=sdk.parent().ok_or_else(||Error::invalid("SDK path has no crates directory"))?;
            std::fs::create_dir(output)?;std::fs::create_dir(output.join("src"))?;
            // JSON strings are also valid TOML basic strings, preventing path interpolation.
            let manifest=format!("[package]\nname = \"semwright-textstats-local\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nsemwright-plugin-sdk = {{ path = {} }}\nsemwright-types = {{ path = {} }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt\"] }}\n",serde_json::to_string(&sdk)?,serde_json::to_string(&parent.join("types"))?);
            create(&output.join("Cargo.toml"),&manifest)?;create(&output.join("src/main.rs"),include_str!("../../../adapters/example-plugin/src/main.rs"))?;
            create(&output.join("README.md"),"# Local textstats plugin\n\nBuild this project, then generate a manifest with scripts/plugin-manifest.py from the Semwright repository. Review the executable digest and permissions before an operator installs it. No code is installed by scaffolding.\n")?;
            print_result(&json!({"created":output,"installed":false,"next":"Build and generate a reviewed digest-bound manifest"}),cli.json)?;
        },
        _=>return Ok(false),
    }Ok(true)
}
async fn run(cli:&Cli)->Result<i32>{
    if local(cli).await?{return Ok(0);}
    let socket=cli.socket.clone().map(Ok).unwrap_or_else(ipc::default_socket)?;
    let ticket=cli.session_file.clone().unwrap_or(socket.with_file_name("cli.session"));
    let mut client=ipc::connect_persistent(&socket,&ticket).await?;
    if let Command::Watch{after}=&cli.command{
        ipc::write_frame(&mut client.stream,&ClientMessage::Subscribe{after:*after}).await?;
        // Never cancel a partially consumed framed read just to send a heartbeat.
        let(mut read,mut write)=client.stream.into_split();let heartbeat=tokio::spawn(async move{let mut timer=tokio::time::interval(Duration::from_secs(30));loop{timer.tick().await;if ipc::write_frame(&mut write,&ClientMessage::Ping).await.is_err(){break;}}});
        let result=loop{let message=tokio::select!{_=tokio::signal::ctrl_c()=>break Ok(0),message=ipc::read_frame::<_,ServerMessage>(&mut read)=>message};match message{Ok(ServerMessage::Event{sequence,event})=>{if let Err(e)=print_result(&json!({"sequence":sequence,"event":event}),true){break Err(e);}},Ok(ServerMessage::Pong)=>(),Ok(ServerMessage::Error{error})=>break Err(error),Ok(_)=>break Err(Error::new(ErrorCode::ProtocolMismatch,"Unexpected event-stream frame")),Err(e)=>break Err(e)}};
        heartbeat.abort();return result;
    }
    if matches!(&cli.command,Command::Recipe{command:Recipe::Test{..}}){let check=client.execute(unique_id(),ExecuteRequest{command:"doctor".into(),args:json!({}),dry_run:false,backend:None}).await?;if check.data.as_ref().and_then(|d|d.get("fake")).and_then(|v|v.as_bool())!=Some(true){return Err(Error::new(ErrorCode::PolicyDenied,"recipe test requires a broker started with --fake; refusing to run on a live desktop"));}}
    let request=request(cli)?.ok_or_else(||Error::new(ErrorCode::Internal,"Command has no request mapping"))?;
    let id=unique_id();let started=std::time::Instant::now();
    let result=tokio::select!{
        value=client.execute(id.clone(),request.clone())=>value,
        _=tokio::signal::ctrl_c()=>{let _=tokio::time::timeout(Duration::from_secs(1),client.cancel(id.clone())).await;Err(Error::new(ErrorCode::Cancelled,"Cancellation sent; a dispatched action may already have taken effect. Do not retry blindly.").uncertain())},
    };
    let envelope=match result{Ok(e)=>e,Err(e)=>Envelope::finish(id,request.command,"transport".into(),started.elapsed(),request.dry_run,Err(e.uncertain()))};
    let code=envelope.error.as_ref().map_or(0,Error::exit_code);print_result(&serde_json::to_value(envelope)?,cli.json)?;Ok(code)
}
#[tokio::main]async fn main(){let cli=Cli::parse();let code=match run(&cli).await{Ok(code)=>code,Err(error)=>{if cli.json{let _=print_result(&json!({"ok":false,"error":error}),true);}else{eprintln!("{error}");}error.exit_code()}};std::process::exit(code);}
