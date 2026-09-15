use crate::{protocol::{self, Handshake, Request, RequestKind, ResponseStatus}, CapabilityFailure, JournalSnapshot, UsnReply, is_lower_hex, lower_hex};
use std::{ffi::{c_void, OsStr}, fs::{File, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, mem::zeroed, os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle}, path::{Path, PathBuf}, ptr::{null, null_mut}, time::{Duration, Instant}};

type Handle = *mut c_void;
const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const FILE_SHARE_READ:u32=1; const FILE_SHARE_WRITE:u32=2; const FILE_SHARE_DELETE:u32=4; const FILE_READ_ATTRIBUTES:u32=0x80; const OPEN_EXISTING:u32=3;
const FILE_ATTRIBUTE_DIRECTORY:u32=0x10; const FILE_ATTRIBUTE_REPARSE_POINT:u32=0x400; const FILE_FLAG_OPEN_REPARSE_POINT:u32=0x0020_0000;
const PIPE_ACCESS_DUPLEX:u32=3; const FILE_FLAG_FIRST_PIPE_INSTANCE:u32=0x0008_0000; const PIPE_NOWAIT:u32=1; const PIPE_REJECT_REMOTE_CLIENTS:u32=8;
const ERROR_PIPE_CONNECTED:i32=535; const ERROR_PIPE_LISTENING:i32=536; const ERROR_NO_DATA:i32=232; const ERROR_CANCELLED:i32=1223;
const TOKEN_QUERY:u32=8; const TOKEN_USER_CLASS:u32=1; const SDDL_REVISION_1:u32=1; const SEE_MASK_NOCLOSEPROCESS:u32=0x40; const SW_HIDE:i32=0; const STILL_ACTIVE:u32=259;
const VOLUME_NAME_GUID:u32=1; const MOVEFILE_REPLACE_EXISTING:u32=1; const MOVEFILE_WRITE_THROUGH:u32=8; const BCRYPT_USE_SYSTEM_PREFERRED_RNG:u32=2;
const CONNECT_TIMEOUT:Duration=Duration::from_secs(45); const MESSAGE_TIMEOUT:Duration=Duration::from_secs(5);

#[repr(C)] #[derive(Clone,Copy,Debug,PartialEq,Eq)] struct ByHandleFileInformation { file_attributes:u32, creation_time_low:u32, creation_time_high:u32, last_access_time_low:u32, last_access_time_high:u32, last_write_time_low:u32, last_write_time_high:u32, volume_serial_number:u32, file_size_high:u32, file_size_low:u32, number_of_links:u32, file_index_high:u32, file_index_low:u32 }
#[repr(C)] struct SecurityAttributes { length:u32, security_descriptor:*mut c_void, inherit_handle:i32 }
#[repr(C)] struct SidAndAttributes { sid:*mut c_void, attributes:u32 }
#[repr(C)] struct TokenUser { user:SidAndAttributes }
#[repr(C)] struct ShellExecuteInfoW { size:u32, mask:u32, hwnd:Handle, verb:*const u16, file:*const u16, parameters:*const u16, directory:*const u16, show:i32, instance:Handle, id_list:*mut c_void, class:*const u16, class_key:Handle, hot_key:u32, icon_or_monitor:Handle, process:Handle }

#[link(name="kernel32")] unsafe extern "system" {
 fn CloseHandle(h:Handle)->i32; fn GetCurrentProcess()->Handle; fn GetCurrentProcessId()->u32; fn GetProcessId(h:Handle)->u32; fn GetExitCodeProcess(h:Handle,c:*mut u32)->i32;
 fn GetFileInformationByHandle(h:Handle,i:*mut ByHandleFileInformation)->i32; fn GetVolumeInformationByHandleW(h:Handle,n:*mut u16,nl:u32,s:*mut u32,m:*mut u32,f:*mut u32,fs:*mut u16,fsl:u32)->i32;
 fn GetFinalPathNameByHandleW(h:Handle,p:*mut u16,l:u32,f:u32)->u32; fn CreateNamedPipeW(n:*const u16,o:u32,m:u32,mi:u32,os:u32,is:u32,t:u32,s:*const SecurityAttributes)->Handle;
 fn ConnectNamedPipe(h:Handle,o:*mut c_void)->i32; fn GetNamedPipeClientProcessId(h:Handle,p:*mut u32)->i32; fn PeekNamedPipe(h:Handle,b:*mut c_void,l:u32,r:*mut u32,a:*mut u32,left:*mut u32)->i32;
 fn ReadFile(h:Handle,b:*mut c_void,l:u32,r:*mut u32,o:*mut c_void)->i32; fn WriteFile(h:Handle,b:*const c_void,l:u32,w:*mut u32,o:*mut c_void)->i32; fn LocalFree(m:*mut c_void)->*mut c_void;
 fn MoveFileExW(a:*const u16,b:*const u16,f:u32)->i32;
}
#[link(name="advapi32")] unsafe extern "system" { fn OpenProcessToken(p:Handle,a:u32,t:*mut Handle)->i32; fn GetTokenInformation(t:Handle,c:u32,i:*mut c_void,l:u32,r:*mut u32)->i32; fn ConvertSidToStringSidW(s:*mut c_void,o:*mut *mut u16)->i32; fn ConvertStringSecurityDescriptorToSecurityDescriptorW(s:*const u16,r:u32,d:*mut *mut c_void,z:*mut u32)->i32; }
#[link(name="shell32")] unsafe extern "system" { fn ShellExecuteExW(i:*mut ShellExecuteInfoW)->i32; }
#[link(name="bcrypt")] unsafe extern "system" { fn BCryptGenRandom(a:*mut c_void,b:*mut u8,l:u32,f:u32)->i32; }

struct OwnedHandle(Handle); unsafe impl Send for OwnedHandle {}
impl OwnedHandle { fn new(h:Handle)->Option<Self>{(!h.is_null()&&h!=INVALID_HANDLE_VALUE).then_some(Self(h))} }
impl Drop for OwnedHandle { fn drop(&mut self){unsafe{CloseHandle(self.0);}} }

#[derive(Clone,Debug,PartialEq,Eq)] pub struct FileIdentity { pub volume_guid:String, pub volume_serial:u64, pub file_id:[u8;16], pub attributes:u32 }
pub struct ProtectedFile { file:File, identity:FileIdentity }
impl ProtectedFile { pub fn identity(&self)->&FileIdentity{&self.identity} pub fn file_mut(&mut self)->&mut File{&mut self.file} pub fn unchanged(&self)->bool{identity_from_handle(self.file.as_raw_handle().cast()).is_some_and(|v|v==self.identity)} }

pub fn open_protected(path:&Path)->Result<ProtectedFile,CapabilityFailure>{
 let file=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(path).map_err(|_|CapabilityFailure::ProtectedHandleUnavailable)?;
 let identity=identity_from_handle(file.as_raw_handle().cast()).ok_or(CapabilityFailure::NonNtfs)?;
 if identity.attributes&FILE_ATTRIBUTE_REPARSE_POINT!=0{return Err(CapabilityFailure::ReparsePoint)}
 if identity.attributes&FILE_ATTRIBUTE_DIRECTORY!=0{return Err(CapabilityFailure::NotRegularFile)}
 Ok(ProtectedFile{file,identity})
}

pub struct CapabilitySession { pipe:OwnedHandle, process:OwnedHandle, nonce:[u8;protocol::NONCE_LEN], volume_guid:[u8;protocol::VOLUME_GUID_LEN] }
impl CapabilitySession {
 pub fn launch<F>(volume_guid:&str, helper_path:&Path, expected_digest:&str, pinned_digest:&str, verify:F)->Result<Self,CapabilityFailure> where F:FnOnce(&mut File)->std::io::Result<String>{
  if !is_lower_hex(expected_digest,64)||expected_digest!=pinned_digest||!is_lower_hex(pinned_digest,64){return Err(CapabilityFailure::HelperIdentityMismatch)}
  let helper_path=helper_path.canonicalize().map_err(|_|CapabilityFailure::HelperMissing)?;
  let mut guard=OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&helper_path).map_err(|_|CapabilityFailure::HelperMissing)?;
  let info=basic_info(guard.as_raw_handle().cast()).ok_or(CapabilityFailure::HelperIdentityMismatch)?;
  if info.file_attributes&(FILE_ATTRIBUTE_REPARSE_POINT|FILE_ATTRIBUTE_DIRECTORY)!=0{return Err(CapabilityFailure::HelperIdentityMismatch)}
  guard.seek(SeekFrom::Start(0)).map_err(|_|CapabilityFailure::HelperIdentityMismatch)?;
  if verify(&mut guard).map_err(|_|CapabilityFailure::HelperIdentityMismatch)?!=pinned_digest{return Err(CapabilityFailure::HelperIdentityMismatch)}
  let mut nonce=[0u8;protocol::NONCE_LEN]; random(&mut nonce)?; let nonce_hex=lower_hex(&nonce); let pipe_name=format!(r"\\.\pipe\BootOptimPandora-USN-v1-{nonce_hex}");
  let descriptor=security_descriptor()?; let pipe_w=wide(OsStr::new(&pipe_name)); let mut sa=SecurityAttributes{length:std::mem::size_of::<SecurityAttributes>() as u32,security_descriptor:descriptor,inherit_handle:0};
  let pipe=OwnedHandle::new(unsafe{CreateNamedPipeW(pipe_w.as_ptr(),PIPE_ACCESS_DUPLEX|FILE_FLAG_FIRST_PIPE_INSTANCE,PIPE_NOWAIT|PIPE_REJECT_REMOTE_CLIENTS,1,4096,4096,MESSAGE_TIMEOUT.as_millis() as u32,&mut sa)}); unsafe{LocalFree(descriptor)}; let pipe=pipe.ok_or(CapabilityFailure::InvalidPipeAcl)?;
  let verb=wide(OsStr::new("runas")); let exe=wide(helper_path.as_os_str()); let params=wide(OsStr::new(&format!("--pipe {pipe_name} --nonce {nonce_hex} --server-pid {}",unsafe{GetCurrentProcessId()})));
  let mut shell=ShellExecuteInfoW{size:std::mem::size_of::<ShellExecuteInfoW>() as u32,mask:SEE_MASK_NOCLOSEPROCESS,hwnd:null_mut(),verb:verb.as_ptr(),file:exe.as_ptr(),parameters:params.as_ptr(),directory:null(),show:SW_HIDE,instance:null_mut(),id_list:null_mut(),class:null(),class_key:null_mut(),hot_key:0,icon_or_monitor:null_mut(),process:null_mut()};
  if unsafe{ShellExecuteExW(&mut shell)}==0{return Err(if last_error()==ERROR_CANCELLED{CapabilityFailure::UacDenied}else{CapabilityFailure::HelperMissing})}
  let process=OwnedHandle::new(shell.process).ok_or(CapabilityFailure::HelperCrash)?; let helper_pid=unsafe{GetProcessId(process.0)}; if helper_pid==0{return Err(CapabilityFailure::HelperCrash)}
  connect(pipe.0,Instant::now()+CONNECT_TIMEOUT)?; let mut client_pid=0; if unsafe{GetNamedPipeClientProcessId(pipe.0,&mut client_pid)}==0||client_pid!=helper_pid{return Err(CapabilityFailure::InvalidPeerPid)}
  let hello=protocol::encode_handshake(Handshake{nonce,pid:unsafe{GetCurrentProcessId()}}); if !write_exact(pipe.0,&hello){return Err(CapabilityFailure::HelperCrash)} let mut ack=[0u8;protocol::HANDSHAKE_LEN]; if !read_exact_timeout(pipe.0,&mut ack,Instant::now()+MESSAGE_TIMEOUT){return Err(CapabilityFailure::Timeout)} let ack=protocol::decode_handshake(&ack).ok_or(CapabilityFailure::MalformedProtocol)?; if ack.nonce!=nonce||ack.pid!=helper_pid{return Err(CapabilityFailure::InvalidPeerPid)}
  drop(guard); Ok(Self{pipe,process,nonce,volume_guid:protocol::volume_guid_bytes(volume_guid).ok_or(CapabilityFailure::MalformedProtocol)?})
 }
 pub fn journal(&mut self)->Result<UsnReply,CapabilityFailure>{self.query(RequestKind::Journal,[0;16])}
 pub fn file(&mut self,file_id:[u8;16])->Result<UsnReply,CapabilityFailure>{self.query(RequestKind::File,file_id)}
 fn query(&mut self,kind:RequestKind,file_id:[u8;16])->Result<UsnReply,CapabilityFailure>{ let mut exit=0; if unsafe{GetExitCodeProcess(self.process.0,&mut exit)}==0||exit!=STILL_ACTIVE{return Err(CapabilityFailure::HelperCrash)} let q=protocol::encode_request(Request{kind,nonce:self.nonce,volume_guid:self.volume_guid,file_id}); if !write_exact(self.pipe.0,&q){return Err(CapabilityFailure::HelperCrash)} let mut b=[0u8;protocol::RESPONSE_LEN]; if !read_exact_timeout(self.pipe.0,&mut b,Instant::now()+MESSAGE_TIMEOUT){return Err(CapabilityFailure::Timeout)} let r=protocol::decode_response(&b).ok_or(CapabilityFailure::MalformedProtocol)?; if r.nonce!=self.nonce{return Err(CapabilityFailure::MalformedProtocol)} if r.status!=ResponseStatus::Ok{return Err(if r.status==ResponseStatus::NonNtfs{CapabilityFailure::NonNtfs}else{CapabilityFailure::Io})} let j=JournalSnapshot{volume_serial:r.volume_serial,journal_id:r.journal_id,first_usn:r.first_usn,lowest_valid_usn:r.lowest_valid_usn,next_usn:r.next_usn}; if !j.valid(){return Err(CapabilityFailure::Io)} Ok(UsnReply{journal:j,file_id:r.file_id,file_usn:r.file_usn}) }
}
impl Drop for CapabilitySession { fn drop(&mut self){ let q=protocol::encode_request(Request{kind:RequestKind::Shutdown,nonce:self.nonce,volume_guid:self.volume_guid,file_id:[0;16]}); let _=write_exact(self.pipe.0,&q); } }

pub fn atomic_replace(path:&Path,bytes:&[u8])->std::io::Result<()> { let mut r=[0u8;16]; random(&mut r).map_err(|_|std::io::Error::other("rng"))?; let tmp=path.with_extension(format!("{}.tmp",lower_hex(&r))); let mut f=OpenOptions::new().write(true).create_new(true).open(&tmp)?; f.write_all(bytes)?; f.sync_all()?; drop(f); let a=wide(tmp.as_os_str()); let b=wide(path.as_os_str()); if unsafe{MoveFileExW(a.as_ptr(),b.as_ptr(),MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)}==0{let e=std::io::Error::last_os_error();let _=std::fs::remove_file(&tmp);return Err(e)} Ok(()) }

fn basic_info(h:Handle)->Option<ByHandleFileInformation>{let mut i=unsafe{zeroed()};if unsafe{GetFileInformationByHandle(h,&mut i)}==0{None}else{Some(i)}}
fn identity_from_handle(h:Handle)->Option<FileIdentity>{let i=basic_info(h)?;let mut serial=0;let mut max=0;let mut flags=0;let mut fs=[0u16;16];if unsafe{GetVolumeInformationByHandleW(h,null_mut(),0,&mut serial,&mut max,&mut flags,fs.as_mut_ptr(),fs.len() as u32)}==0{return None} if i.volume_serial_number!=serial{return None} let n=fs.iter().position(|v|*v==0).unwrap_or(fs.len());if String::from_utf16_lossy(&fs[..n])!="NTFS"{return None} let mut p=[0u16;32768];let w=unsafe{GetFinalPathNameByHandleW(h,p.as_mut_ptr(),p.len() as u32,VOLUME_NAME_GUID)} as usize;if w==0||w>=p.len(){return None} let full=String::from_utf16_lossy(&p[..w]);let guid=full.get(..protocol::VOLUME_GUID_LEN)?.to_owned();protocol::volume_guid_bytes(&guid)?;let raw=((i.file_index_high as u64)<<32)|i.file_index_low as u64;let mut id=[0u8;16];id[..8].copy_from_slice(&raw.to_le_bytes());Some(FileIdentity{volume_guid:guid,volume_serial:serial as u64,file_id:id,attributes:i.file_attributes})}
fn random(out:&mut[u8])->Result<(),CapabilityFailure>{if unsafe{BCryptGenRandom(null_mut(),out.as_mut_ptr(),out.len() as u32,BCRYPT_USE_SYSTEM_PREFERRED_RNG)}==0{Ok(())}else{Err(CapabilityFailure::Io)}}
fn security_descriptor()->Result<*mut c_void,CapabilityFailure>{let mut token=null_mut();if unsafe{OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)}==0{return Err(CapabilityFailure::InvalidPipeAcl)}let token=OwnedHandle::new(token).ok_or(CapabilityFailure::InvalidPipeAcl)?;let mut needed=0;unsafe{GetTokenInformation(token.0,TOKEN_USER_CLASS,null_mut(),0,&mut needed)};if needed==0{return Err(CapabilityFailure::InvalidPipeAcl)}let words=(needed as usize+std::mem::size_of::<usize>()-1)/std::mem::size_of::<usize>();let mut buf=vec![0usize;words];if unsafe{GetTokenInformation(token.0,TOKEN_USER_CLASS,buf.as_mut_ptr().cast(),needed,&mut needed)}==0{return Err(CapabilityFailure::InvalidPipeAcl)}let user=unsafe{&*(buf.as_ptr().cast::<TokenUser>())};let mut sidp:*mut u16=null_mut();if unsafe{ConvertSidToStringSidW(user.user.sid,&mut sidp)}==0||sidp.is_null(){return Err(CapabilityFailure::InvalidPipeAcl)}let mut n=0;unsafe{while *sidp.add(n)!=0{n+=1}}let sid=String::from_utf16_lossy(unsafe{std::slice::from_raw_parts(sidp,n)});unsafe{LocalFree(sidp.cast())};let sddl=wide(OsStr::new(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})")));let mut d=null_mut();if unsafe{ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),SDDL_REVISION_1,&mut d,null_mut())}==0||d.is_null(){return Err(CapabilityFailure::InvalidPipeAcl)}Ok(d)}
fn connect(h:Handle,deadline:Instant)->Result<(),CapabilityFailure>{while Instant::now()<deadline{if unsafe{ConnectNamedPipe(h,null_mut())}!=0{return Ok(())}match last_error(){ERROR_PIPE_CONNECTED=>return Ok(()),ERROR_PIPE_LISTENING|ERROR_NO_DATA=>std::thread::sleep(Duration::from_millis(10)),_=>return Err(CapabilityFailure::MalformedProtocol)}}Err(CapabilityFailure::Timeout)}
fn read_exact_timeout(h:Handle,o:&mut[u8],deadline:Instant)->bool{while Instant::now()<deadline{let mut a=0;let ok=unsafe{PeekNamedPipe(h,null_mut(),0,null_mut(),&mut a,null_mut())};if ok==0{if matches!(last_error(),ERROR_NO_DATA|ERROR_PIPE_LISTENING){std::thread::sleep(Duration::from_millis(5));continue}return false}if a<o.len() as u32{std::thread::sleep(Duration::from_millis(5));continue}let mut r=0;return unsafe{ReadFile(h,o.as_mut_ptr().cast(),o.len() as u32,&mut r,null_mut())}!=0&&r as usize==o.len()}false}
fn write_exact(h:Handle,i:&[u8])->bool{let mut w=0;unsafe{WriteFile(h,i.as_ptr().cast(),i.len() as u32,&mut w,null_mut())}!=0&&w as usize==i.len()}
fn last_error()->i32{std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)} fn wide(v:&OsStr)->Vec<u16>{v.encode_wide().chain(Some(0)).collect()}

#[cfg(test)] mod tests { use super::*; #[test] fn protected_handle_blocks_write_delete(){let d=std::env::temp_dir().join(format!("bootoptim-cap-{}",std::process::id()));let _=std::fs::create_dir_all(&d);let p=d.join("f");std::fs::write(&p,b"abcdefgh").unwrap();let Ok(f)=open_protected(&p) else{return};assert!(OpenOptions::new().write(true).open(&p).is_err());assert!(std::fs::remove_file(&p).is_err());assert!(f.unchanged());drop(f);let _=std::fs::remove_dir_all(d);} }
