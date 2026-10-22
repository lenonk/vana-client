//! Sequential CAB extraction. The `cab` crate restarts a folder's decompression for
//! every file it opens; MSI cabinets pack thousands of files per folder, so this reads
//! each folder once, front to back. Handles stored and MSZIP folders only.

use anyhow::{Result, bail};
use md5::{Digest, Md5};
use std::fs;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

struct Folder {
    offset: u32,
    blocks: u16,
    compression: u16,
}

struct Entry {
    name: String,
    size: u32,
    folder_offset: u32,
    folder: u16,
}

pub struct Extracted {
    pub name: String,
    pub path: PathBuf,
    pub md5: [u8; 16],
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn read_cstr<R: Read>(r: &mut R) -> io::Result<String> {
    let mut s = Vec::new();
    let mut b = [0u8; 1];
    loop {
        r.read_exact(&mut b)?;
        if b[0] == 0 {
            return Ok(String::from_utf8_lossy(&s).into_owned());
        }
        s.push(b[0]);
    }
}

/// Uncompressed bytes of one folder, block by block.
struct FolderStream<'a, R> {
    r: &'a mut R,
    blocks_left: u16,
    data_reserve: usize,
    compression: u16,
    inflate: flate2::Decompress,
    dict: Vec<u8>,
    block: Vec<u8>,
    pos: usize,
}

impl<R: Read> FolderStream<'_, R> {
    fn next_block(&mut self) -> io::Result<bool> {
        if self.blocks_left == 0 {
            return Ok(false);
        }
        self.blocks_left -= 1;
        let mut hdr = [0u8; 8];
        self.r.read_exact(&mut hdr)?;
        let (packed, unpacked) = (u16le(&hdr, 4) as usize, u16le(&hdr, 6) as usize);
        let mut skip = vec![0u8; self.data_reserve];
        self.r.read_exact(&mut skip)?;
        let mut data = vec![0u8; packed];
        self.r.read_exact(&mut data)?;
        self.pos = 0;
        if self.compression == 0 {
            self.block = data;
            return Ok(true);
        }
        if data.len() < 2 || &data[..2] != b"CK" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad MSZIP block signature"));
        }
        // Each MSZIP block is its own deflate stream seeded with the previous 32 KiB;
        // a stored block replays that history into the window.
        self.inflate.reset(false);
        if !self.dict.is_empty() {
            let n = self.dict.len() as u16;
            let mut seed = vec![0u8];
            seed.extend_from_slice(&n.to_le_bytes());
            seed.extend_from_slice(&(!n).to_le_bytes());
            seed.extend_from_slice(&self.dict);
            let mut sink = Vec::with_capacity(self.dict.len());
            self.inflate
                .decompress_vec(&seed, &mut sink, flate2::FlushDecompress::Sync)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }
        let mut out = Vec::with_capacity(unpacked);
        self.inflate
            .decompress_vec(&data[2..], &mut out, flate2::FlushDecompress::Finish)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if out.len() != unpacked {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "MSZIP block size mismatch"));
        }
        let keep = out.len().min(0x8000);
        let total = self.dict.len() + keep;
        if total > 0x8000 {
            self.dict.drain(..total - 0x8000);
        }
        self.dict.extend_from_slice(&out[out.len() - keep..]);
        self.block = out;
        Ok(true)
    }
}

impl<R: Read> Read for FolderStream<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos == self.block.len() {
            if !self.next_block()? {
                return Ok(0);
            }
        }
        let n = buf.len().min(self.block.len() - self.pos);
        buf[..n].copy_from_slice(&self.block[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Extracts every file `dest_for` maps to a path, reporting uncompressed bytes as they are written.
pub fn extract_all<R: Read + Seek>(
    reader: R,
    mut dest_for: impl FnMut(&str) -> Option<PathBuf>,
    mut on_bytes: impl FnMut(u64),
) -> Result<Vec<Extracted>> {
    let mut r = BufReader::with_capacity(1 << 20, reader);
    let mut hdr = [0u8; 36];
    r.read_exact(&mut hdr)?;
    if &hdr[..4] != b"MSCF" {
        bail!("not a cabinet");
    }
    let (files_offset, n_folders, n_files, flags) = (u32le(&hdr, 16), u16le(&hdr, 26), u16le(&hdr, 28), u16le(&hdr, 30));
    let (mut folder_reserve, mut data_reserve) = (0usize, 0usize);
    if flags & 4 != 0 {
        let mut res = [0u8; 4];
        r.read_exact(&mut res)?;
        folder_reserve = res[2] as usize;
        data_reserve = res[3] as usize;
        io::copy(&mut (&mut r).take(u16le(&res, 0) as u64), &mut io::sink())?;
    }
    if flags & 3 != 0 {
        bail!("multi-volume cabinets are not supported");
    }
    let mut folders = Vec::new();
    for _ in 0..n_folders {
        let mut f = vec![0u8; 8 + folder_reserve];
        r.read_exact(&mut f)?;
        folders.push(Folder { offset: u32le(&f, 0), blocks: u16le(&f, 4), compression: u16le(&f, 6) & 0x0f });
    }
    if let Some(f) = folders.iter().find(|f| f.compression > 1) {
        bail!("unsupported cabinet compression type {}", f.compression);
    }
    r.seek(SeekFrom::Start(files_offset as u64))?;
    let mut entries = Vec::with_capacity(n_files as usize);
    for _ in 0..n_files {
        let mut f = [0u8; 16];
        r.read_exact(&mut f)?;
        let name = read_cstr(&mut r)?;
        entries.push(Entry { name, size: u32le(&f, 0), folder_offset: u32le(&f, 4), folder: u16le(&f, 8) });
    }

    let mut done = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    for (index, folder) in folders.iter().enumerate() {
        let mut files: Vec<&Entry> = entries.iter().filter(|e| e.folder as usize == index).collect();
        files.sort_by_key(|e| e.folder_offset);
        r.seek(SeekFrom::Start(folder.offset as u64))?;
        let mut stream = FolderStream {
            r: &mut r,
            blocks_left: folder.blocks,
            data_reserve,
            compression: folder.compression,
            inflate: flate2::Decompress::new(false),
            dict: Vec::with_capacity(0x8000),
            block: Vec::new(),
            pos: 0,
        };
        let mut at = 0u64;
        // WiX stores identical files once; later entries point back at the same bytes.
        let mut stored: std::collections::HashMap<(u32, u32), (PathBuf, [u8; 16])> = Default::default();
        for e in files {
            if (e.folder_offset as u64) < at {
                let Some((src, md5)) = stored.get(&(e.folder_offset, e.size)).cloned() else {
                    bail!("{}: overlaps another file in the cabinet", e.name);
                };
                if let Some(path) = dest_for(&e.name) {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&src, &path)?;
                    on_bytes(e.size as u64);
                    done.push(Extracted { name: e.name.clone(), path, md5 });
                }
                continue;
            }
            let skip = e.folder_offset as u64 - at;
            io::copy(&mut (&mut stream).take(skip), &mut io::sink())?;
            at = e.folder_offset as u64 + e.size as u64;
            let mut part = (&mut stream).take(e.size as u64);
            let Some(path) = dest_for(&e.name) else {
                io::copy(&mut part, &mut io::sink())?;
                continue;
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = BufWriter::with_capacity(1 << 20, fs::File::create(&path)?);
            let mut md5 = Md5::new();
            let mut written = 0u64;
            loop {
                let n = part.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                md5.update(&buf[..n]);
                out.write_all(&buf[..n])?;
                written += n as u64;
                on_bytes(n as u64);
            }
            out.flush()?;
            if written != e.size as u64 {
                bail!("{}: cabinet ended after {written} of {} bytes", e.name, e.size);
            }
            let md5: [u8; 16] = md5.finalize().into();
            stored.insert((e.folder_offset, e.size), (path.clone(), md5));
            done.push(Extracted { name: e.name.clone(), path, md5 });
        }
    }
    Ok(done)
}
