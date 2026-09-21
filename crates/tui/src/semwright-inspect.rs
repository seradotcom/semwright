//! Read-only terminal inspector. No hidden action/approval or independent backend path.
use clap::Parser;
use crossterm::{event::{self,Event,KeyCode,KeyEventKind},execute,terminal::{enable_raw_mode,disable_raw_mode,EnterAlternateScreen,LeaveAlternateScreen}};
use ratatui::{backend::CrosstermBackend,Terminal,widgets::{Block,Borders,Paragraph,Tabs},layout::{Layout,Constraint},text::{Line,Text}};
use semwright_protocol::{connect_persistent,default_socket};
use semwright_types::*;
use serde_json::{Value,json};
use std::{path::PathBuf,time::Duration,io::{self,IsTerminal}};
#[derive(Parser)]#[command(version,about="Read-only semantic desktop inspector: tab to switch, r to refresh, / to filter, q to exit")]
struct Args{#[arg(long)]socket:Option<PathBuf>,#[arg(long)]session_file:Option<PathBuf>}
struct Restore;
impl Drop for Restore{fn drop(&mut self){let _=disable_raw_mode();let _=execute!(io::stdout(),LeaveAlternateScreen);}}
const PANES:[(&str,&str);7]=[("Doctor","doctor"),("Windows","window.list"),("Apps","app.list"),("UI","ui.snapshot"),("Policy","capabilities.list"),("Audit","audit.tail"),("Plugins","plugin.list")];
fn escaped(text:&str)->String{text.chars().flat_map(|c|{if c.is_control()&&c!='\n'&&c!='\t'||matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}'){c.escape_default().collect::<Vec<_>>()}else{vec![c]}}).collect()}
async fn fetch(socket:&std::path::Path,ticket:&std::path::Path,pane:usize)->Result<Value>{let mut client=connect_persistent(socket,ticket).await?;let args=if pane==3{json!({"max_nodes":200,"max_depth":5})}else if pane==5{json!({"limit":50})}else{json!({})};let envelope=client.execute(unique_id(),ExecuteRequest{command:PANES[pane].1.into(),args,dry_run:false,backend:None}).await?;serde_json::to_value(envelope).map_err(Into::into)}
async fn run()->Result<()> {
    let args=Args::parse();if !io::stdout().is_terminal()||!io::stdin().is_terminal(){return Err(Error::invalid("Inspector requires a terminal; use computerctl --json for pipes"));}
    let socket=args.socket.map(Ok).unwrap_or_else(default_socket)?;let ticket=args.session_file.unwrap_or(socket.with_file_name("cli.session"));
    enable_raw_mode()?;let _restore=Restore;execute!(io::stdout(),EnterAlternateScreen)?;let mut terminal=Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let(mut pane,mut scroll,mut query,mut search)=(0usize,0u16,String::new(),false);let mut body=String::from("Press r to refresh.");
    let(mut task,mut requested):(Option<tokio::task::JoinHandle<Result<Value>>>,bool)=(None,true);
    loop{
        if requested{if let Some(old)=task.take(){old.abort();}let s=socket.clone();let t=ticket.clone();task=Some(tokio::spawn(async move{fetch(&s,&t,pane).await}));requested=false;body="Loading from broker…".into();}
        if task.as_ref().is_some_and(|t|t.is_finished()){if let Some(done)=task.take(){body=match done.await{Ok(Ok(value))=>escaped(&serde_json::to_string_pretty(&value)?),Ok(Err(error))=>escaped(&error.to_string()),Err(_)=>"Inspector request cancelled".into()};}}
        let filtered=if query.is_empty(){body.clone()}else{body.lines().filter(|l|l.to_lowercase().contains(&query.to_lowercase())).collect::<Vec<_>>().join("\n")};
        terminal.draw(|frame|{let area=Layout::vertical([Constraint::Length(3),Constraint::Min(2),Constraint::Length(2)]).split(frame.area());let titles=PANES.iter().map(|(name,_)|Line::from(*name)).collect::<Vec<_>>();frame.render_widget(Tabs::new(titles).select(pane).block(Block::default().borders(Borders::ALL).title("Semwright / read-only inspector")),area[0]);frame.render_widget(Paragraph::new(Text::from(filtered.as_str())).scroll((scroll,0)).block(Block::default().borders(Borders::ALL).title(PANES[pane].1)),area[1]);frame.render_widget(Paragraph::new(format!("Tab/←/→ panes  ↑/↓ scroll  r refresh  / search  Esc clear  q quit\n{}{}",if search{"Search: "}else{"Filter: "},escaped(&query))),area[2]);})?;
        if event::poll(Duration::from_millis(50))?{if let Event::Key(key)=event::read()?{if key.kind!=KeyEventKind::Press{continue;}
            if search{match key.code{KeyCode::Esc|KeyCode::Enter=>search=false,KeyCode::Backspace=>{query.pop();},KeyCode::Char(c)if query.len()<256&&!c.is_control()=>query.push(c),_=>()}scroll=0;continue;}
            match key.code{KeyCode::Char('q')=>break,KeyCode::Tab|KeyCode::Right=>{pane=(pane+1)%PANES.len();scroll=0;requested=true;},KeyCode::Left=>{pane=(pane+PANES.len()-1)%PANES.len();scroll=0;requested=true;},KeyCode::Down=>scroll=scroll.saturating_add(1),KeyCode::Up=>scroll=scroll.saturating_sub(1),KeyCode::PageDown=>scroll=scroll.saturating_add(15),KeyCode::PageUp=>scroll=scroll.saturating_sub(15),KeyCode::Char('r')=>requested=true,KeyCode::Char('/')=>search=true,KeyCode::Esc=>{query.clear();scroll=0;},_=>()}
        }}
    }
    if let Some(task)=task{task.abort();}Ok(())
}
#[tokio::main]async fn main(){if let Err(error)=run().await{eprintln!("semwright-inspect: {error}");std::process::exit(error.exit_code());}}
#[cfg(test)]mod tests{use super::*;#[test]fn terminal_injection_is_escaped(){assert!(!escaped("\u{1b}]52;c;secret\u{7}").contains('\u{1b}'));assert!(!escaped("\u{202e}").contains('\u{202e}'));}}
