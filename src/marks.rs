use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MARKS_SCHEMA: &str = "termpdf.marks.v1";

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct NamedMark {
    pub ref_id: String,
    pub page_index: usize,
    pub relative_x: f32,
    pub relative_y: f32,
    pub zoom_percent: u16,
}

#[derive(Clone, Debug)]
pub struct MarkStore {
    path: PathBuf,
    document_key: String,
    source_path: String,
    source_sha256: String,
    loaded_marks: BTreeMap<String, NamedMark>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MarksFile {
    schema: String,
    documents: BTreeMap<String, DocumentMarks>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DocumentMarks {
    source_path: String,
    source_sha256: String,
    marks: BTreeMap<String, NamedMark>,
}

impl Default for MarksFile {
    fn default() -> Self {
        Self {
            schema: MARKS_SCHEMA.to_string(),
            documents: BTreeMap::new(),
        }
    }
}

impl MarkStore {
    pub fn open(pdf_path: &Path, path: PathBuf) -> io::Result<(Self, BTreeMap<String, NamedMark>)> {
        let mut store = Self::for_document(pdf_path, path)?;
        let marks = store.load()?;
        Ok((store, marks))
    }

    pub(crate) fn for_document(pdf_path: &Path, path: PathBuf) -> io::Result<Self> {
        let source_sha256 = source_sha256(pdf_path)?;
        Ok(Self::for_document_with_sha256(
            pdf_path,
            path,
            source_sha256,
        ))
    }

    pub(crate) fn for_document_with_sha256(
        pdf_path: &Path,
        path: PathBuf,
        source_sha256: String,
    ) -> Self {
        let canonical_path = fs::canonicalize(pdf_path).unwrap_or_else(|_| pdf_path.to_path_buf());
        let source_path = canonical_path.to_string_lossy().into_owned();
        let mut document_key = Sha256::new();
        document_key.update(path_identity_bytes(&canonical_path));
        document_key.update(b"\0");
        document_key.update(source_sha256.as_bytes());
        let document_key = format!("{:x}", document_key.finalize());
        Self {
            path,
            document_key,
            source_path,
            source_sha256,
            loaded_marks: BTreeMap::new(),
        }
    }

    pub(crate) fn load(&mut self) -> io::Result<BTreeMap<String, NamedMark>> {
        let file = read_marks_file(&self.path)?;
        let marks = file
            .documents
            .get(&self.document_key)
            .map(|document| document.marks.clone())
            .unwrap_or_default();
        self.loaded_marks = marks.clone();
        Ok(marks)
    }

    pub fn save(
        &mut self,
        marks: &BTreeMap<String, NamedMark>,
    ) -> io::Result<BTreeMap<String, NamedMark>> {
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "marks file has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path.with_extension("lock"))?;
        lock_file.lock()?;

        let mut file = read_marks_file(&self.path)?;
        let mut merged_marks = file
            .documents
            .get(&self.document_key)
            .map(|document| document.marks.clone())
            .unwrap_or_default();
        for name in self.loaded_marks.keys() {
            if !marks.contains_key(name) {
                merged_marks.remove(name);
            }
        }
        for (name, mark) in marks {
            if self.loaded_marks.get(name) != Some(mark) {
                merged_marks.insert(name.clone(), mark.clone());
            }
        }
        file.documents.insert(
            self.document_key.clone(),
            DocumentMarks {
                source_path: self.source_path.clone(),
                source_sha256: self.source_sha256.clone(),
                marks: merged_marks.clone(),
            },
        );

        let temp_path = self
            .path
            .with_extension(format!("tmp.{}", std::process::id()));
        let mut content = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
        content.push(b'\n');
        if let Err(error) = fs::write(&temp_path, content) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp_path, &self.path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }

        self.loaded_marks = merged_marks.clone();
        Ok(merged_marks)
    }

    pub(crate) fn document_key(&self) -> &str {
        &self.document_key
    }
}

pub fn default_marks_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library/Application Support/termpdf/marks.json"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(state_home) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(state_home).join("termpdf/marks.json"));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".local/state/termpdf/marks.json"))
    }
}

fn read_marks_file(path: &Path) -> io::Result<MarksFile> {
    let content = match fs::read(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(MarksFile::default()),
        Err(error) => return Err(error),
    };
    let file = serde_json::from_slice::<MarksFile>(&content).map_err(io::Error::other)?;
    if file.schema != MARKS_SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported marks schema '{}'", file.schema),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn path_identity_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

pub(crate) fn source_sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
