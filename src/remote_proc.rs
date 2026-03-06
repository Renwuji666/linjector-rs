use std::path::{Path, PathBuf};

use crate::{remote_mem::RemoteMem, remote_module::RemoteModule, InjectionError};

pub(crate) struct RemoteProc {
    pid: i32,
    pub mem: RemoteMem,
}

#[derive(Clone, Debug)]
struct ProcMap {
    start: usize,
    size: usize,
    offset: usize,
    filename: Option<PathBuf>,
}

impl ProcMap {
    fn start(&self) -> usize {
        self.start
    }

    fn size(&self) -> usize {
        self.size
    }

    fn filename(&self) -> Option<&Path> {
        self.filename.as_deref()
    }
}

impl RemoteProc {
    pub fn new(pid: i32) -> Result<Self, InjectionError> {
        let mem = RemoteMem::new(pid)?;
        Ok(Self { pid, mem })
    }

    fn parse_map_line(line: &str) -> Option<ProcMap> {
        let mut parts = line.split_whitespace();

        let range = parts.next()?;
        let _perms = parts.next()?;
        let offset_hex = parts.next()?;
        let _dev = parts.next()?;
        let _inode = parts.next()?;
        let filename = parts.next().map(PathBuf::from);

        let (start_hex, end_hex) = range.split_once('-')?;
        let start = usize::from_str_radix(start_hex, 16).ok()?;
        let end = usize::from_str_radix(end_hex, 16).ok()?;
        if end < start {
            return None;
        }

        let offset = usize::from_str_radix(offset_hex, 16).ok()?;

        Some(ProcMap {
            start,
            size: end - start,
            offset,
            filename,
        })
    }

    fn maps(&self) -> Result<Vec<ProcMap>, InjectionError> {
        let maps_path = format!("/proc/{}/maps", self.pid);
        let maps_raw = std::fs::read(maps_path).map_err(|_| InjectionError::RemoteProcessError)?;
        let maps_text = String::from_utf8_lossy(&maps_raw);
        let maps = maps_text
            .lines()
            .filter_map(Self::parse_map_line)
            .collect::<Vec<_>>();

        if maps.is_empty() {
            return Err(InjectionError::RemoteProcessError);
        }

        Ok(maps)
    }

    fn maps_by_name(&self, name: &str) -> Result<Vec<ProcMap>, InjectionError> {
        let maps = self.maps()?;
        let mut maps_by_name: Vec<ProcMap> = Vec::new();
        for map in maps {
            match map.filename() {
                None => continue,
                Some(filename) => {
                    if filename.ends_with(name) {
                        maps_by_name.push(map);
                    }
                }
            }
        }

        if maps_by_name.is_empty() {
            return Err(InjectionError::ModuleNotFound);
        }

        Ok(maps_by_name)
    }

    fn module_bytes(&self, module_name: &str) -> Result<Vec<u8>, InjectionError> {
        let maps = self.maps_by_name(module_name)?;
        let mut module_bytes: Vec<u8> = Vec::new();
        for map in maps {
            // debug!("map: {:?}", map);
            module_bytes.resize(map.offset, 0);
            let mut buf = self.mem.read(map.start(), map.size())?;
            module_bytes.append(&mut buf);
        }

        Ok(module_bytes)
    }

    pub fn module(&self, module_name: &str) -> Result<RemoteModule, InjectionError> {
        let maps = self.maps_by_name(module_name)?;
        let filename = maps[0]
            .filename()
            .ok_or(InjectionError::ModuleNotFound)?
            .to_str()
            .ok_or(InjectionError::ModuleNotFound)?;
        Ok(RemoteModule::new(
            filename,
            maps[0].start(),
            self.module_bytes(module_name)?,
        ))
    }
}
