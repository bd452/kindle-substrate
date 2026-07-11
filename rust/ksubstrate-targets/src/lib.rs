use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

pub const TWEAKS_ROOT: &str = "/var/local/kmc/tweaks";
pub const PACKAGES_ROOT: &str = "/mnt/us/kmc/kpm/packages";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSpec { Builtin(String), Kpm { package: String, path: String } }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Init { Constructor, Entrypoint }
#[derive(Clone, Debug)]
pub struct Manifest { pub id: String, pub library: String, pub initialization: Init, pub targets: Vec<TargetSpec> }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestartClass { Framework, NextLaunch }
#[derive(Clone, Debug)]
pub struct ResolvedTarget { pub id: String, pub executable: PathBuf, pub restart: RestartClass, pub package: Option<String> }
#[derive(Clone, Debug)]
pub struct LibraryIdentity { pub id: String, pub library: PathBuf, pub init: Init, pub dev: u64, pub ino: u64, pub size: u64, pub digest: u64 }
#[derive(Clone, Debug)]
pub struct PlanTarget { pub target: ResolvedTarget, pub alias: PathBuf, pub libraries: Vec<LibraryIdentity> }
#[derive(Clone, Debug)]
pub struct SessionPlan { pub generation: u64, pub platform: String, pub targets: Vec<PlanTarget> }

pub fn platform() -> &'static str { if Path::new("/lib/ld-linux-armhf.so.3").exists() { "kindlehf" } else { "kindlepw2" } }
pub fn valid_id(value: &str) -> bool { !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) }
pub fn regular_file(path: &Path) -> Result<(), String> { let m=fs::symlink_metadata(path).map_err(|e|format!("stat {}: {e}",path.display()))?; if m.file_type().is_symlink() || !m.is_file(){Err(format!("not a regular file: {}",path.display()))}else{Ok(())} }

pub fn parse_manifest(input: &str) -> Result<Manifest, String> {
    if json_number(input,"manifest_version") != Some(2) { return Err("manifest_version 2 is required".to_owned()); }
    let id=json_string(input,"id").ok_or_else(||"manifest id is required".to_owned())?; if !valid_id(&id){return Err("invalid manifest id".to_owned())}
    let library=json_string(input,"library").ok_or_else(||"manifest library is required".to_owned())?; if !simple_name(&library){return Err("unsafe manifest library".to_owned())}
    let initialization=match json_string(input,"initialization").as_deref(){Some("constructor")=>Init::Constructor,Some("entrypoint")=>Init::Entrypoint,_=>return Err("manifest initialization must be constructor or entrypoint".to_owned())};
    let targets=parse_targets(input)?; if targets.is_empty(){return Err("manifest has no targets".to_owned())}
    Ok(Manifest{id,library,initialization,targets})
}

pub fn resolve(spec:&TargetSpec, platform:&str)->Result<ResolvedTarget,String>{match spec{
    TargetSpec::Builtin(name)=>builtin(name),
    TargetSpec::Kpm{package,path}=>resolve_kpm(package,path,platform),
}}
fn builtin(name:&str)->Result<ResolvedTarget,String>{let (path,restart)=match name{ "pillow"=>("/usr/bin/pillow",RestartClass::Framework),"appmgrd"=>("/usr/bin/appmgrd",RestartClass::Framework),_=>return Err(format!("unknown built-in target: {name}"))}; reject_blacklisted(Path::new(path))?; regular_executable(Path::new(path))?;Ok(ResolvedTarget{id:format!("builtin:{name}"),executable:path.into(),restart,package:None})}
fn resolve_kpm(package:&str,relative:&str,platform:&str)->Result<ResolvedTarget,String>{if !valid_id(package){return Err("invalid KPM package id".to_owned())}let expanded=relative.replace("{platform}",platform);if expanded.contains('{')||!safe_relative(&expanded){return Err("unsafe KPM target path".to_owned())}let root=Path::new(PACKAGES_ROOT).join(package);let manifest=root.join("manifest.json");let text=fs::read_to_string(&manifest).map_err(|e|format!("read target package manifest: {e}"))?;if !json_bool(&text,"ksubstrate_target_lifecycle").unwrap_or(false){return Err(format!("KPM package {package} has not opted into Substrate target lifecycle"))}let executable=root.join(&expanded);if !executable.starts_with(&root){return Err("KPM target escaped package root".to_owned())}regular_executable(&executable)?;reject_blacklisted(&executable)?;Ok(ResolvedTarget{id:format!("kpm:{package}:{relative}"),executable,restart:RestartClass::NextLaunch,package:Some(package.to_owned())})}
fn regular_executable(path:&Path)->Result<(),String>{regular_file(path)?;let m=fs::metadata(path).map_err(|e|e.to_string())?;if m.mode()&0o111==0{Err(format!("not executable: {}",path.display()))}else{Ok(())}}
fn safe_relative(value:&str)->bool{let p=Path::new(value);!p.is_absolute()&&!value.is_empty()&&p.components().all(|c|matches!(c,Component::Normal(_))) }
fn simple_name(value:&str)->bool{!value.is_empty()&&Path::new(value).components().count()==1&&value!="."&&value!=".."}
pub fn reject_blacklisted(path:&Path)->Result<(),String>{let name=path.file_name().and_then(|v|v.to_str()).unwrap_or("");if matches!(name,"powerd"|"sshd"|"dbus-daemon"|"dbus"|"otav3"|"otaupd"|"mmcqd"|"wpa_supplicant"|"dhcpd"|"ksubstrated"|"ksubstrate"){Err(format!("blacklisted target: {}",path.display()))}else{Ok(())}}

pub fn library_identity(id:String, library:PathBuf, init:Init)->Result<LibraryIdentity,String>{regular_file(&library)?;let m=fs::metadata(&library).map_err(|e|e.to_string())?;let bytes=fs::read(&library).map_err(|e|format!("read tweak library: {e}"))?;Ok(LibraryIdentity{id,library,init,dev:m.dev(),ino:m.ino(),size:m.size(),digest:fnv64(&bytes)})}
pub fn verify_library(identity:&LibraryIdentity)->Result<(),String>{let current=library_identity(identity.id.clone(),identity.library.clone(),identity.init.clone())?;if current.dev==identity.dev&&current.ino==identity.ino&&current.size==identity.size&&current.digest==identity.digest{Ok(())}else{Err(format!("tweak library changed since session plan: {}",identity.library.display()))}}
fn fnv64(bytes:&[u8])->u64{bytes.iter().fold(0xcbf29ce484222325u64,|hash,b|(hash^u64::from(*b)).wrapping_mul(0x100000001b3))}

pub fn encode_plan(plan:&SessionPlan)->String{let mut out=format!("version\t1\ngeneration\t{}\nplatform\t{}\n",plan.generation,plan.platform);for target in &plan.targets{out.push_str(&format!("target\t{}\t{}\t{}\t{}\n",target.target.id,target.target.executable.display(),target.alias.display(),match target.target.restart{RestartClass::Framework=>"framework",RestartClass::NextLaunch=>"next-launch"}));for library in &target.libraries{out.push_str(&format!("library\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",target.target.id,library.id,library.library.display(),match library.init{Init::Constructor=>"constructor",Init::Entrypoint=>"entrypoint"},library.dev,library.ino,library.size,library.digest));}}out}
pub fn decode_plan(input:&str)->Result<SessionPlan,String>{let mut generation=None;let mut platform=None;let mut targets=Vec::<PlanTarget>::new();let mut pending=Vec::<(String,LibraryIdentity)>::new();for line in input.lines(){let f:Vec<_>=line.split('\t').collect();match f.as_slice(){["version","1"]=>{},["generation",v]=>generation=Some(v.parse().map_err(|_|"invalid plan generation")?),["platform",v]=>platform=Some((*v).to_owned()),["target",id,exe,alias,restart]=>targets.push(PlanTarget{target:ResolvedTarget{id:(*id).to_owned(),executable:PathBuf::from(exe),restart:if *restart=="framework"{RestartClass::Framework}else if *restart=="next-launch"{RestartClass::NextLaunch}else{return Err("invalid plan restart class".to_owned())},package:None},alias:PathBuf::from(alias),libraries:Vec::new()}),["library",target,id,path,init,dev,ino,size,digest]=>pending.push(((*target).to_owned(),LibraryIdentity{id:(*id).to_owned(),library:PathBuf::from(path),init:if *init=="constructor"{Init::Constructor}else if *init=="entrypoint"{Init::Entrypoint}else{return Err("invalid plan init".to_owned())},dev:dev.parse().map_err(|_|"invalid plan dev")?,ino:ino.parse().map_err(|_|"invalid plan ino")?,size:size.parse().map_err(|_|"invalid plan size")?,digest:digest.parse().map_err(|_|"invalid plan digest")?})),_=>return Err("malformed session plan".to_owned())}}for (target,library) in pending{targets.iter_mut().find(|entry|entry.target.id==target).ok_or_else(||"library references unknown target".to_owned())?.libraries.push(library);}Ok(SessionPlan{generation:generation.ok_or_else(||"missing plan generation".to_owned())?,platform:platform.ok_or_else(||"missing plan platform".to_owned())?,targets})}

fn parse_targets(input:&str)->Result<Vec<TargetSpec>,String>{let body=array_body(input,"targets").ok_or_else(||"manifest targets must be an array".to_owned())?;let mut targets=Vec::new();let mut i=0;let bytes=body.as_bytes();while i<bytes.len(){while i<bytes.len()&&(bytes[i].is_ascii_whitespace()||bytes[i]==b','){i+=1}if i>=bytes.len(){break}if bytes[i]==b'"'{let end=body[i+1..].find('"').ok_or_else(||"unterminated target string".to_owned())?+i+1;let name=&body[i+1..end];if !name.contains('\\'){targets.push(TargetSpec::Builtin(name.to_owned()));}else{return Err("escaped target names are unsupported".to_owned())}i=end+1}else if bytes[i]==b'{'{let end=object_end(body,i).ok_or_else(||"unterminated target object".to_owned())?;let object=&body[i..=end];if json_string(object,"kind").as_deref()!=Some("kpm"){return Err("unknown target object kind".to_owned())}let package=json_string(object,"package").ok_or_else(||"KPM target package required".to_owned())?;let path=json_string(object,"path").ok_or_else(||"KPM target path required".to_owned())?;targets.push(TargetSpec::Kpm{package,path});i=end+1}else{return Err("invalid target entry".to_owned())}}Ok(targets)}
fn object_end(input:&str,start:usize)->Option<usize>{let mut depth=0usize;let mut quoted=false;let mut escaped=false;for(index,byte)in input.as_bytes().iter().enumerate().skip(start){if quoted{if escaped{escaped=false}else if *byte==b'\\'{escaped=true}else if *byte==b'"'{quoted=false};continue}match byte{b'"'=>quoted=true,b'{'=>depth+=1,b'}'=>{depth=depth.checked_sub(1)?;if depth==0{return Some(index)}},_=>{}}}None}
pub fn json_string(input:&str,key:&str)->Option<String>{let marker=format!("\"{key}\"");let(_,rest)=input.split_once(&marker)?;let rest=rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;let end=rest.find('"')?;let value=&rest[..end];(!value.contains('\\')).then(||value.to_owned())}
fn json_number(input:&str,key:&str)->Option<u64>{let marker=format!("\"{key}\"");let(_,rest)=input.split_once(&marker)?;rest.trim_start().strip_prefix(':')?.trim_start().split(|c:char|!c.is_ascii_digit()).next()?.parse().ok()}
fn json_bool(input:&str,key:&str)->Option<bool>{let marker=format!("\"{key}\"");let(_,rest)=input.split_once(&marker)?;match rest.trim_start().strip_prefix(':')?.trim_start(){v if v.starts_with("true")=>Some(true),v if v.starts_with("false")=>Some(false),_=>None}}
fn array_body<'a>(input:&'a str,key:&str)->Option<&'a str>{let marker=format!("\"{key}\"");let(_,rest)=input.split_once(&marker)?;let rest=rest.trim_start().strip_prefix(':')?.trim_start();let start=rest.find('[')?;let mut d=0usize;for(i,b)in rest.as_bytes().iter().enumerate().skip(start){match b{b'['=>d+=1,b']'=>{d=d.checked_sub(1)?;if d==0{return Some(&rest[start+1..i])}},_=>{}}}None}

#[cfg(test)]
mod tests{use super::*;#[test]fn parses_v2(){let m=parse_manifest(r#"{"manifest_version":2,"id":"com.example.x","library":"tweak.so","initialization":"constructor","targets":["pillow",{"kind":"kpm","package":"com.example.app","path":"bin/{platform}/app"}]}"#).unwrap();assert_eq!(m.targets.len(),2)}#[test]fn rejects_v1(){assert!(parse_manifest(r#"{"manifest_version":1}"#).is_err())}#[test]fn plan_round_trips(){let plan=SessionPlan{generation:7,platform:"kindlepw2".to_owned(),targets:vec![PlanTarget{target:ResolvedTarget{id:"builtin:pillow".to_owned(),executable:"/usr/bin/pillow".into(),restart:RestartClass::Framework,package:None},alias:"/tmp/original/pillow".into(),libraries:vec![LibraryIdentity{id:"com.example.x".to_owned(),library:"/tmp/tweak.so".into(),init:Init::Constructor,dev:1,ino:2,size:3,digest:4}]}]};let decoded=decode_plan(&encode_plan(&plan)).unwrap();assert_eq!(decoded.generation,7);assert_eq!(decoded.targets[0].libraries[0].digest,4)}}
