//! Versioned deterministic recipes: exact typed bindings, no shell interpolation.
use async_trait::async_trait;
use semwright_types::*;
use serde::{Serialize,Deserialize};
use serde_json::{Value,json};
use std::{collections::{BTreeMap,BTreeSet},time::Duration};
use tokio_util::sync::CancellationToken;

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum ValueType{String,Number,Integer,Boolean,Object,Array}
impl ValueType{
    pub fn accepts(self,value:&Value)->bool{match self{Self::String=>value.is_string(),Self::Number=>value.is_number(),Self::Integer=>value.is_i64()||value.is_u64(),Self::Boolean=>value.is_boolean(),Self::Object=>value.is_object(),Self::Array=>value.is_array()}}
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input{pub kind:ValueType,#[serde(default)]pub secret:bool,#[serde(default)]pub default:Option<Value>}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output{pub kind:ValueType,pub value:Value,#[serde(default)]pub secret:bool}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe{
    pub version:u32,pub name:String,
    #[serde(default)]pub description:String,
    #[serde(default)]pub inputs:BTreeMap<String,Input>,
    pub steps:Vec<Step>,
    #[serde(default)]pub outputs:BTreeMap<String,Output>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step{
    pub id:String,pub command:String,
    #[serde(default="empty_object")]pub args:Value,
    pub timeout_ms:u64,
    #[serde(default)]pub retry:Retry,
    #[serde(default)]pub when:Option<Assertion>,
    #[serde(default)]pub assertions:Vec<Assertion>,
}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retry{pub attempts:u32,pub backoff_ms:u64}
impl Default for Retry{fn default()->Self{Self{attempts:1,backoff_ms:100}}}
#[derive(Debug,Clone,Copy,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Operator{Equals,NotEquals,Truthy,Exists,CountEquals}
#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion{pub left:Value,pub op:Operator,#[serde(default)]pub right:Value}
#[async_trait]
pub trait Executor:Send+Sync{
    fn describe(&self,command:&str)->Result<CommandDescriptor>;
    async fn execute(&self,request:ExecuteRequest,cancellation:CancellationToken)->Result<Value>;
}
pub fn parse(text:&str)->Result<Recipe>{
    if text.len()>262_144{return Err(Error::new(ErrorCode::ResourceExhausted,"Recipe document exceeds 256 KiB"));}
    // No user-defined YAML tags or arbitrary object constructors are supported.
    serde_yaml_ng::from_str(text).map_err(|_|Error::invalid("Invalid recipe YAML/JSON or unknown fields"))
}
fn identifier(value:&str)->bool{!value.is_empty()&&value.len()<=80&&value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'))}
impl Recipe{
    pub fn validate(&self,executor:&dyn Executor)->Result<Value>{
        if self.version!=1{return Err(Error::new(ErrorCode::ProtocolMismatch,"Recipe version must be 1"));}
        if !identifier(&self.name)||self.steps.is_empty()||self.steps.len()>64||self.inputs.len()>64||self.outputs.len()>64{return Err(Error::invalid("Invalid recipe name or recipe budget exceeded"));}
        for (name,input)in &self.inputs{
            if !identifier(name)||input.default.as_ref().is_some_and(|v|!input.kind.accepts(v)){return Err(Error::invalid("Input name/default does not match its declared type"));}
        }
        let mut seen=BTreeSet::new();let mut plan=vec![];
        for step in &self.steps{
            if !identifier(&step.id)||!seen.insert(step.id.clone()){return Err(Error::invalid("Step ids must be unique identifiers"));}
            if step.command.starts_with("recipe."){return Err(Error::invalid("Nested recipe execution is not supported"));}
            let descriptor=executor.describe(&step.command)?;
            if step.timeout_ms==0||step.timeout_ms>descriptor.timeout_ms{return Err(Error::invalid("Step timeout must be positive and no greater than the command timeout"));}
            if step.retry.attempts==0||step.retry.attempts>5||step.retry.backoff_ms>5000{return Err(Error::invalid("Retry budget must be 1..5 attempts and at most 5000ms backoff"));}
            if step.retry.attempts>1&&!matches!(descriptor.idempotency,Idempotency::ReadOnly|Idempotency::Idempotent){return Err(Error::invalid("Retry is forbidden for non-idempotent or destructive commands"));}
            validate_bindings(&step.args,0)?;
            if step.assertions.len()>16{return Err(Error::invalid("Too many assertions"));}
            for assertion in step.assertions.iter().chain(step.when.iter()){validate_bindings(&assertion.left,0)?;validate_bindings(&assertion.right,0)?;}
            plan.push(json!({"step":step.id,"command":step.command,"risk":descriptor.risk,"requires":descriptor.requires,"timeout_ms":step.timeout_ms,"attempts":step.retry.attempts,"condition":step.when.is_some()}));
        }
        for (name,output)in &self.outputs{if !identifier(name){return Err(Error::invalid("Invalid output name"));}validate_bindings(&output.value,0)?;}
        Ok(json!({"valid":true,"name":self.name,"version":1,"steps":plan,"note":"Bindings and argument schemas are validated again against actual step results before execution"}))
    }
    pub async fn run(&self,executor:&dyn Executor,inputs:Value,dry_run:bool,cancellation:CancellationToken)->Result<Value>{
        let plan=self.validate(executor)?;
        let supplied=inputs.as_object().ok_or_else(||Error::invalid("Recipe inputs must be an object"))?;
        if supplied.keys().any(|key|!self.inputs.contains_key(key)){return Err(Error::invalid("Undeclared recipe input"));}
        let mut normalized=serde_json::Map::new();
        for (name,declaration)in &self.inputs{
            let value=supplied.get(name).cloned().or_else(||declaration.default.clone()).ok_or_else(||Error::invalid("Required recipe input missing"))?;
            if !declaration.kind.accepts(&value){return Err(Error::invalid("Recipe input type mismatch"));}normalized.insert(name.clone(),value);
        }
        if dry_run{return Ok(json!({"dry_run":true,"plan":plan,"side_effects":false,"bindings":"step-derived values unresolved; this is not an execution success"}));}
        let mut state=json!({"inputs":normalized,"steps":{}});let mut summary=vec![];
        let mut tainted:BTreeSet<String>=self.inputs.iter().filter(|(_,i)|i.secret).map(|(n,_)|format!("/inputs/{n}")).collect();
        for step in &self.steps{
            if cancellation.is_cancelled(){return Err(Error::new(ErrorCode::Cancelled,"Recipe cancelled before next step").recipe_progress(summary.len(),&step.id));}
            if let Some(condition)=&step.when{if !check(condition,&state)?{state["steps"][&step.id]=json!({"skipped":true});summary.push(json!({"step":step.id,"skipped":true}));continue;}}
            let secret_flow=is_tainted(&step.args,&tainted)||step.when.as_ref().is_some_and(|c|is_tainted(&c.left,&tainted)||is_tainted(&c.right,&tainted))||executor.describe(&step.command)?.risk==Risk::SecretAccess;
            if secret_flow{tainted.insert(format!("/steps/{}",step.id));}
            let args=resolve(&step.args,&state,0).map_err(|e|e.recipe_progress(summary.len(),&step.id))?;
            let mut attempt=0;
            let data=loop{
                attempt+=1;let child=cancellation.child_token();
                let request=ExecuteRequest{command:step.command.clone(),args:args.clone(),dry_run:false,backend:None};
                let result=tokio::select!{
                    _=cancellation.cancelled()=>{child.cancel();Err(Error::new(ErrorCode::Cancelled,"Recipe step cancelled").uncertain())},
                    value=tokio::time::timeout(Duration::from_millis(step.timeout_ms),executor.execute(request,child.clone()))=>match value{Ok(value)=>value,Err(_)=>{child.cancel();Err(Error::new(ErrorCode::Timeout,"Recipe step timed out").uncertain())}},
                };
                match result{
                    Ok(value)=>break value,
                    Err(error)=>{
                        let retryable=error.outcome_known&&matches!(error.code,ErrorCode::BackendFailed|ErrorCode::Unavailable);
                        if attempt>=step.retry.attempts||!retryable{return Err(error.recipe_progress(summary.len(),&step.id));}
                        tokio::select!{_=cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Recipe cancelled during backoff")),_=tokio::time::sleep(Duration::from_millis(step.retry.backoff_ms))=>()};
                    }
                }
            };
            state["steps"][&step.id]=data;
            for assertion in &step.assertions{if !check(assertion,&state)?{return Err(Error::new(ErrorCode::Conflict,"Recipe assertion failed; remaining steps were not executed").recipe_progress(summary.len()+1,&step.id));}}
            summary.push(json!({"step":step.id,"ok":true,"attempts":attempt}));
        }
        let mut outputs=serde_json::Map::new();
        for (name,declaration)in &self.outputs{
            let value=resolve(&declaration.value,&state,0)?;
            if !declaration.kind.accepts(&value){return Err(Error::new(ErrorCode::BackendFailed,"Recipe output violates its declared type"));}
            outputs.insert(name.clone(),if declaration.secret||is_tainted(&declaration.value,&tainted){Value::String("[REDACTED]".into())}else{value});
        }
        Ok(json!({"completed":true,"steps":summary,"outputs":outputs}))
    }
}
fn validate_bindings(value:&Value,depth:usize)->Result<()>{
    if depth>32{return Err(Error::invalid("Recipe expression nesting exceeds 32"));}
    match value{
        Value::Object(map)=>{
            if let Some(pointer)=map.get("$var"){
                if map.len()!=1||!pointer.as_str().is_some_and(|s|s.len()<=512&&(s.starts_with("/inputs/")||s.starts_with("/steps/"))){return Err(Error::invalid("A binding is exactly {\"$var\":\"/inputs/name\"} or a /steps JSON pointer"));}
            }else{for v in map.values(){validate_bindings(v,depth+1)?;}}
        }
        Value::Array(values)=>{if values.len()>2000{return Err(Error::invalid("Recipe array exceeds budget"));}for v in values{validate_bindings(v,depth+1)?;}},_=>(),
    }Ok(())
}
pub fn resolve(value:&Value,state:&Value,depth:usize)->Result<Value>{
    validate_bindings(value,depth)?;
    match value{
        Value::Object(map) if map.contains_key("$var")=>{
            let path=map["$var"].as_str().ok_or_else(||Error::invalid("Invalid variable path"))?;
            state.pointer(path).cloned().ok_or_else(||Error::new(ErrorCode::NotFound,"Recipe binding refers to a missing input or prior step result"))
        }
        Value::Object(map)=>{let mut out=serde_json::Map::new();for(k,v)in map{out.insert(k.clone(),resolve(v,state,depth+1)?);}Ok(Value::Object(out))},
        Value::Array(values)=>values.iter().map(|v|resolve(v,state,depth+1)).collect::<Result<Vec<_>>>().map(Value::Array),
        _=>Ok(value.clone()),
    }
}
fn check(assertion:&Assertion,state:&Value)->Result<bool>{
    let left=resolve(&assertion.left,state,0);
    if matches!(assertion.op,Operator::Exists){return Ok(left.is_ok_and(|v|!v.is_null()));}
    let left=left?;let right=resolve(&assertion.right,state,0)?;
    Ok(match assertion.op{
        Operator::Equals=>left==right,Operator::NotEquals=>left!=right,
        Operator::Truthy=>left==Value::Bool(true),Operator::Exists=>!left.is_null(),
        Operator::CountEquals=>left.as_array().is_some_and(|a|Some(a.len()as u64)==right.as_u64()),
    })
}
#[cfg(test)]mod tests{
    use super::*;use std::sync::atomic::{AtomicUsize,Ordering};use proptest::prelude::*;
    struct Fake{calls:AtomicUsize}
    #[async_trait]impl Executor for Fake{
        fn describe(&self,command:&str)->Result<CommandDescriptor>{Ok(CommandDescriptor{name:command.into(),version:"1".into(),description:String::new(),input_schema:json!({}),output_schema:json!({}),requires:vec!["desktop.observe".into()],risk:Risk::ReadOnly,idempotency:if command=="destructive"{Idempotency::Destructive}else{Idempotency::ReadOnly},timeout_ms:1000,dry_run:true,interactive_consent:false,backends:vec!["fake".into()]})}
        async fn execute(&self,_:ExecuteRequest,_:CancellationToken)->Result<Value>{self.calls.fetch_add(1,Ordering::SeqCst);Ok(json!({"value":42}))}
    }
    fn recipe()->Recipe{parse("version: 1\nname: sample\nsteps:\n  - id: first\n    command: read\n    timeout_ms: 1000\noutputs:\n  answer:\n    kind: integer\n    value: {$var: /steps/first/value}\n").unwrap()}
    #[tokio::test]async fn deterministic_output(){let f=Fake{calls:AtomicUsize::new(0)};let out=recipe().run(&f,json!({}),false,CancellationToken::new()).await.unwrap();assert_eq!(out["outputs"]["answer"],42);assert_eq!(f.calls.load(Ordering::SeqCst),1);}
    #[tokio::test]async fn dry_run_has_no_calls(){let f=Fake{calls:AtomicUsize::new(0)};recipe().run(&f,json!({}),true,CancellationToken::new()).await.unwrap();assert_eq!(f.calls.load(Ordering::SeqCst),0);}
    #[test]fn no_destructive_retries(){let mut r=recipe();r.steps[0].command="destructive".into();r.steps[0].retry.attempts=2;assert!(r.validate(&Fake{calls:AtomicUsize::new(0)}).is_err());}
    #[test]fn strings_are_not_shell_templates(){let value=json!("$(rm -rf /) ${input}");assert_eq!(resolve(&value,&json!({}),0).unwrap(),value);}
    #[test]fn unknown_fields_fail(){assert!(parse("version: 1\nname: x\nsteps: []\nshell: true").is_err());}
    #[test]fn mixed_binding_objects_fail(){assert!(resolve(&json!({"$var":"/inputs/x","other":1}),&json!({"inputs":{"x":1}}),0).is_err());}
    #[tokio::test]async fn cancelled_recipe_does_not_run(){let f=Fake{calls:AtomicUsize::new(0)};let cancel=CancellationToken::new();cancel.cancel();assert!(recipe().run(&f,json!({}),false,cancel).await.is_err());assert_eq!(f.calls.load(Ordering::SeqCst),0);}
    proptest!{#[test]fn binding_preserves_types(n in any::<i64>()){let state=json!({"inputs":{"n":n}});prop_assert_eq!(resolve(&json!({"$var":"/inputs/n"}),&state,0).unwrap(),json!(n));}}
}

fn is_tainted(value:&Value,roots:&BTreeSet<String>)->bool{
    match value{
        Value::Object(map)=>if let Some(path)=map.get("$var").and_then(Value::as_str){roots.iter().any(|root|path==root||path.strip_prefix(root).is_some_and(|tail|tail.starts_with('/')))}else{map.values().any(|v|is_tainted(v,roots))},
        Value::Array(values)=>values.iter().any(|v|is_tainted(v,roots)),_=>false,
    }
}
