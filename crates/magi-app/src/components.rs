//! Optional downloads: parts some features need that MacroMagi does not ship,
//! installed into (and removed from) the data folder on request.
//!
//! | Part | For | From |
//! | --- | --- | --- |
//! | ONNX Runtime | faster AI models on the CPU | Microsoft's GitHub releases |
//! | ONNX Runtime for graphics cards | AI models on any DirectX 12 graphics card (Windows) | Microsoft's NuGet packages |
//! | Python | the Python engine, without installing Python (Windows) | python.org |
//! | PyTorch | training models (coming to the Magic tab) | pip, into a Python |
//!
//! Each part lives in its own folder under `data/components`. A part in use
//! cannot be deleted on Windows (its files are locked); it is then marked and
//! removed the next time MacroMagi starts.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// ONNX Runtime for the CPU, and the last release built for DirectML.
const ORT_VERSION: &str = "1.31.0";
const ORT_DIRECTML_VERSION: &str = "1.24.4";
const DIRECTML_VERSION: &str = "1.15.4";
const PYTHON_VERSION: &str = "3.13.14";
/// Marks a folder to be deleted at the next start.
const REMOVE_MARK: &str = ".remove";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Component {
    OnnxRuntime,
    OnnxRuntimeGpu,
    Python,
    PyTorch,
}

impl Component {
    pub const ALL: [Component; 4] =
        [Component::OnnxRuntime, Component::OnnxRuntimeGpu, Component::Python, Component::PyTorch];

    pub fn name(self) -> &'static str {
        match self {
            Component::OnnxRuntime => "ONNX Runtime",
            Component::OnnxRuntimeGpu => "ONNX Runtime for graphics cards",
            Component::Python => "Python",
            Component::PyTorch => "PyTorch",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Component::OnnxRuntime => {
                "Runs AI models faster on the CPU than the built-in engine. AI models work without it."
            }
            Component::OnnxRuntimeGpu => {
                "Runs AI models on a graphics card (any DirectX 12 card, through DirectML). Also does what ONNX \
                 Runtime does."
            }
            Component::Python => {
                "Python for the Python engine and Python Script blocks, on computers that do not have it."
            }
            Component::PyTorch => {
                "For training and fine-tuning models, coming to the Magic tab. Installed into the Python MacroMagi \
                 uses."
            }
        }
    }

    /// About how much is downloaded.
    pub fn size(self) -> &'static str {
        match self {
            Component::OnnxRuntime => "about 15 MB",
            Component::OnnxRuntimeGpu => "about 40 MB",
            Component::Python => "about 11 MB",
            Component::PyTorch => "about 250 MB",
        }
    }

    /// Why it cannot be downloaded on this system, if it cannot.
    pub fn unavailable(self) -> Option<&'static str> {
        match self {
            Component::OnnxRuntimeGpu if !cfg!(windows) => Some(if cfg!(target_os = "macos") {
                "On macOS, ONNX Runtime uses the graphics hardware by itself."
            } else {
                "On Linux, graphics card support needs CUDA; install ONNX Runtime with CUDA yourself."
            }),
            Component::OnnxRuntime if cfg!(all(target_os = "macos", target_arch = "x86_64")) => {
                Some("ONNX Runtime no longer offers builds for Intel Macs; the built-in engine is used.")
            }
            Component::Python if !cfg!(windows) => {
                Some("Python is part of most Linux and macOS systems; install it from python.org if it is missing.")
            }
            _ => None,
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Component::OnnxRuntime => "onnxruntime",
            Component::OnnxRuntimeGpu => "onnxruntime-gpu",
            Component::Python => "python",
            Component::PyTorch => "pytorch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Downloaded by MacroMagi.
    Installed {
        bytes: u64,
        detail: String,
    },
    /// Present without MacroMagi's help.
    OnSystem(String),
    Missing,
    /// Removed, but some files were in use: gone after a restart.
    RemovedAfterRestart,
}

#[derive(Clone)]
pub struct Components {
    root: PathBuf,
    app_dir: PathBuf,
}

fn folder_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| {
                    let path = e.path();
                    if path.is_dir() { folder_size(&path) } else { e.metadata().map_or(0, |m| m.len()) }
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Downloads a file, reporting progress in percent.
fn download(url: &str, progress: &dyn Fn(String)) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url).call().map_err(|e| format!("could not download {url}: {e}"))?;
    let total: Option<u64> =
        response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    let mut reader = response.body_mut().as_reader();
    let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut chunk = vec![0u8; 256 * 1024];
    let mut last = 0;
    loop {
        let read = reader.read(&mut chunk).map_err(|e| format!("the download stopped: {e}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        let megabytes = bytes.len() / (1024 * 1024);
        if megabytes != last {
            last = megabytes;
            progress(match total {
                Some(total) => format!("Downloading… {}%", bytes.len() as u64 * 100 / total.max(1)),
                None => format!("Downloading… {megabytes} MB"),
            });
        }
    }
    Ok(bytes)
}

/// Writes the files of a ZIP archive whose paths end with one of `wanted`
/// into `into`, under their own file name. Returns how many were written.
fn unzip_files(archive: &[u8], wanted: &[&str], into: &Path) -> Result<usize, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| e.to_string())?;
    let mut written = 0;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.replace('\\', "/");
        if let Some(wanted) = wanted.iter().find(|w| name.ends_with(*w)) {
            let file = wanted.rsplit('/').next().unwrap_or(wanted);
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            std::fs::write(into.join(file), bytes).map_err(|e| e.to_string())?;
            written += 1;
        }
    }
    Ok(written)
}

/// Unpacks a whole ZIP archive into `into`.
fn unzip_all(archive: &[u8], into: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| e.to_string())?;
    zip.extract(into).map_err(|e| e.to_string())
}

/// The file of a `.tgz` archive whose name starts with `prefix` (the real
/// library, not its links), saved as `save_as`.
#[cfg(not(windows))]
fn untar_library(archive: &[u8], prefix: &str, save_as: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut tar = tar::Archive::new(decoder);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        let file = path.rsplit('/').next().unwrap_or(&path).to_owned();
        if entry.header().entry_type().is_file() && file.starts_with(prefix) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            return std::fs::write(save_as, bytes).map_err(|e| e.to_string());
        }
    }
    Err("the library was not found in the download".into())
}

/// Runs a Python command, returning what it printed.
fn python_output(python: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(python);
    command.args(args).env("PYTHONDONTWRITEBYTECODE", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output().map_err(|e| format!("could not run {}: {e}", python.display()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        let last = error.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it failed");
        Err(last.trim().to_owned())
    }
}

impl Components {
    pub fn new(data_dir: &Path, app_dir: &Path) -> Self {
        Self { root: data_dir.join("components"), app_dir: app_dir.to_owned() }
    }

    fn dir(&self, component: Component) -> PathBuf {
        self.root.join(component.folder())
    }

    /// Deletes what was marked for removal while it was in use. Call at start,
    /// before anything is loaded.
    pub fn clean_up(&self) {
        for component in Component::ALL {
            let dir = self.dir(component);
            if dir.join(REMOVE_MARK).exists() {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }

    fn library_name() -> &'static str {
        if cfg!(windows) {
            "onnxruntime.dll"
        } else if cfg!(target_os = "macos") {
            "libonnxruntime.dylib"
        } else {
            "libonnxruntime.so"
        }
    }

    fn installed(&self, component: Component) -> bool {
        let dir = self.dir(component);
        if dir.join(REMOVE_MARK).exists() {
            return false;
        }
        match component {
            Component::OnnxRuntime | Component::OnnxRuntimeGpu => dir.join(Self::library_name()).is_file(),
            Component::Python => dir.join("python.exe").is_file(),
            Component::PyTorch => false,
        }
    }

    /// The ONNX Runtime library to use, and whether it can use graphics cards:
    /// the graphics build if it is installed, else the CPU one, else one next
    /// to MacroMagi.
    pub fn onnxruntime(&self) -> Option<(PathBuf, bool)> {
        if self.installed(Component::OnnxRuntimeGpu) {
            return Some((self.dir(Component::OnnxRuntimeGpu).join(Self::library_name()), true));
        }
        if self.installed(Component::OnnxRuntime) {
            return Some((self.dir(Component::OnnxRuntime).join(Self::library_name()), cfg!(target_os = "macos")));
        }
        let beside = self.app_dir.join(Self::library_name());
        beside.is_file().then_some((beside, false))
    }

    /// DirectML's library, which has to be loaded before ONNX Runtime asks for it.
    pub fn directml(&self) -> Option<PathBuf> {
        let path = self.dir(Component::OnnxRuntimeGpu).join("DirectML.dll");
        (self.installed(Component::OnnxRuntimeGpu) && path.is_file()).then_some(path)
    }

    /// The Python downloaded by MacroMagi.
    pub fn python(&self) -> Option<PathBuf> {
        let path = self.dir(Component::Python).join("python.exe");
        self.installed(Component::Python).then_some(path)
    }

    /// Where a part stands. `python` is the interpreter PyTorch is looked for in.
    /// May take a moment for PyTorch (it starts Python).
    pub fn status(&self, component: Component, python: Option<&Path>) -> Status {
        let dir = self.dir(component);
        if dir.join(REMOVE_MARK).exists() {
            return Status::RemovedAfterRestart;
        }
        match component {
            Component::PyTorch => {
                let Some(python) = python else { return Status::Missing };
                match python_output(python, &["-c", "import torch; print(torch.__version__)"]) {
                    Ok(version) => {
                        let ours = self.python().is_some_and(|p| p == python);
                        let detail = format!("version {version}, in {}", python.display());
                        if ours { Status::Installed { bytes: 0, detail } } else { Status::OnSystem(detail) }
                    }
                    Err(_) => Status::Missing,
                }
            }
            _ if self.installed(component) => {
                let detail = match component {
                    Component::OnnxRuntime => format!("version {ORT_VERSION}"),
                    Component::OnnxRuntimeGpu => format!("version {ORT_DIRECTML_VERSION}, DirectML {DIRECTML_VERSION}"),
                    Component::Python => format!("version {PYTHON_VERSION}"),
                    Component::PyTorch => String::new(),
                };
                Status::Installed { bytes: folder_size(&dir), detail }
            }
            Component::OnnxRuntime => {
                let beside = self.app_dir.join(Self::library_name());
                if beside.is_file() {
                    Status::OnSystem(format!("next to MacroMagi: {}", beside.display()))
                } else {
                    Status::Missing
                }
            }
            _ => Status::Missing,
        }
    }

    /// Downloads and installs a part. Blocks: run it off the UI thread.
    pub fn install(
        &self,
        component: Component,
        python: Option<&Path>,
        progress: &dyn Fn(String),
    ) -> Result<String, String> {
        if let Some(reason) = component.unavailable() {
            return Err(reason.to_owned());
        }
        let dir = self.dir(component);
        if component != Component::PyTorch {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        let result = self.install_into(component, &dir, python, progress);
        if result.is_err() && component != Component::PyTorch {
            let _ = std::fs::remove_dir_all(&dir);
        }
        result
    }

    fn install_into(
        &self,
        component: Component,
        dir: &Path,
        python: Option<&Path>,
        progress: &dyn Fn(String),
    ) -> Result<String, String> {
        match component {
            Component::OnnxRuntime => {
                let base = format!("https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}");
                #[cfg(windows)]
                {
                    let archive = download(&format!("{base}/onnxruntime-win-x64-{ORT_VERSION}.zip"), progress)?;
                    progress("Unpacking…".into());
                    let found =
                        unzip_files(&archive, &["lib/onnxruntime.dll", "lib/onnxruntime_providers_shared.dll"], dir)?;
                    if found == 0 {
                        return Err("the download did not contain ONNX Runtime".into());
                    }
                }
                #[cfg(target_os = "linux")]
                {
                    let archive = download(&format!("{base}/onnxruntime-linux-x64-{ORT_VERSION}.tgz"), progress)?;
                    progress("Unpacking…".into());
                    untar_library(&archive, "libonnxruntime.so.", &dir.join(Self::library_name()))?;
                }
                #[cfg(target_os = "macos")]
                {
                    let archive = download(&format!("{base}/onnxruntime-osx-arm64-{ORT_VERSION}.tgz"), progress)?;
                    progress("Unpacking…".into());
                    untar_library(&archive, "libonnxruntime.", &dir.join(Self::library_name()))?;
                }
                let _ = base;
                Ok(format!("ONNX Runtime {ORT_VERSION} installed. Restart MacroMagi to use it."))
            }
            Component::OnnxRuntimeGpu => {
                let nuget = "https://api.nuget.org/v3-flatcontainer";
                let ort = download(
                    &format!(
                        "{nuget}/microsoft.ml.onnxruntime.directml/{ORT_DIRECTML_VERSION}/microsoft.ml.onnxruntime.directml.{ORT_DIRECTML_VERSION}.nupkg"
                    ),
                    progress,
                )?;
                progress("Unpacking…".into());
                let found = unzip_files(
                    &ort,
                    &[
                        "runtimes/win-x64/native/onnxruntime.dll",
                        "runtimes/win-x64/native/onnxruntime_providers_shared.dll",
                    ],
                    dir,
                )?;
                if found == 0 {
                    return Err("the download did not contain ONNX Runtime".into());
                }
                let directml = download(
                    &format!(
                        "{nuget}/microsoft.ai.directml/{DIRECTML_VERSION}/microsoft.ai.directml.{DIRECTML_VERSION}.nupkg"
                    ),
                    progress,
                )?;
                progress("Unpacking DirectML…".into());
                if unzip_files(&directml, &["bin/x64-win/DirectML.dll"], dir)? == 0 {
                    return Err("the download did not contain DirectML".into());
                }
                Ok("ONNX Runtime for graphics cards installed. Restart MacroMagi to use it.".into())
            }
            Component::Python => {
                let archive = download(
                    &format!(
                        "https://www.python.org/ftp/python/{PYTHON_VERSION}/python-{PYTHON_VERSION}-embed-amd64.zip"
                    ),
                    progress,
                )?;
                progress("Unpacking…".into());
                unzip_all(&archive, dir)?;
                // Let the private Python find packages installed into it.
                for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "_pth") {
                        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                        let text = text.replace("#import site", "import site");
                        let text = if text.contains("Lib\\site-packages") {
                            text
                        } else {
                            format!("{text}\nLib\\site-packages\n")
                        };
                        std::fs::write(&path, text).map_err(|e| e.to_string())?;
                    }
                }
                Ok(format!(
                    "Python {PYTHON_VERSION} installed. Choose it under Settings → Engine, or leave the choice on automatic."
                ))
            }
            Component::PyTorch => {
                let python = python.ok_or("no Python to install PyTorch into: download Python first")?;
                // A private Python has no pip until it is given one.
                if python_output(python, &["-m", "pip", "--version"]).is_err() {
                    progress("Setting up pip…".into());
                    let script = download("https://bootstrap.pypa.io/get-pip.py", progress)?;
                    let path = self.root.join("get-pip.py");
                    std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
                    std::fs::write(&path, script).map_err(|e| e.to_string())?;
                    let result = python_output(python, &[&path.to_string_lossy(), "--no-warn-script-location"]);
                    let _ = std::fs::remove_file(&path);
                    result.map_err(|e| format!("could not set up pip: {e}"))?;
                }
                progress("Downloading and installing PyTorch (this takes a while)…".into());
                python_output(
                    python,
                    &[
                        "-m",
                        "pip",
                        "install",
                        "--disable-pip-version-check",
                        "--no-warn-script-location",
                        "torch",
                        "--index-url",
                        "https://download.pytorch.org/whl/cpu",
                    ],
                )
                .map_err(|e| format!("pip could not install PyTorch: {e}"))?;
                Ok(format!("PyTorch installed into {}.", python.display()))
            }
        }
    }

    /// Removes a part. Returns a message to show.
    pub fn uninstall(&self, component: Component, python: Option<&Path>) -> Result<String, String> {
        if component == Component::PyTorch {
            let python = python.ok_or("there is no Python to remove PyTorch from")?;
            python_output(python, &["-m", "pip", "uninstall", "-y", "--disable-pip-version-check", "torch"])
                .map_err(|e| format!("pip could not remove PyTorch: {e}"))?;
            return Ok(format!("PyTorch removed from {}.", python.display()));
        }
        let dir = self.dir(component);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(format!("{} removed.", component.name())),
            Err(_) if dir.exists() => {
                // In use: finish at the next start.
                std::fs::write(dir.join(REMOVE_MARK), b"").map_err(|e| e.to_string())?;
                Ok(format!("{} is in use; it is removed when MacroMagi restarts.", component.name()))
            }
            Err(_) => Ok(format!("{} removed.", component.name())),
        }
    }
}

/// Every Python interpreter that can be found: MacroMagi's own, one next to
/// it, and the ones installed on the system. Starts programs: call it off the
/// UI thread.
pub fn find_pythons(components: &Components, app_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(ours) = components.python() {
        candidates.push(ours);
    }
    let beside = if cfg!(windows) {
        app_dir.join("python").join("python.exe")
    } else {
        app_dir.join("python").join("bin").join("python3")
    };
    candidates.push(beside);
    #[cfg(windows)]
    {
        // The py launcher lists every installed Python.
        if let Ok(output) = Command::new("py").arg("-0p").output() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if let Some(path) = line.split_whitespace().last().filter(|p| p.to_lowercase().ends_with(".exe")) {
                    candidates.push(PathBuf::from(path));
                }
            }
        }
    }
    let finder = if cfg!(windows) { "where" } else { "which" };
    for name in ["python", "python3"] {
        let mut command = Command::new(finder);
        if !cfg!(windows) {
            command.arg("-a");
        }
        if let Ok(output) = command.arg(name).output() {
            candidates.extend(String::from_utf8_lossy(&output.stdout).lines().map(|l| PathBuf::from(l.trim())));
        }
    }
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    for path in candidates {
        // The Windows Store placeholder only opens the Store.
        if !path.is_file() || path.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if found.iter().any(|(p, _)| p.canonicalize().unwrap_or_else(|_| p.clone()) == key) {
            continue;
        }
        if let Ok(version) = python_output(&path, &["-c", "import sys; print('%d.%d.%d' % sys.version_info[:3])"]) {
            let ok = version.split('.').take(2).map(|n| n.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
            if ok.len() == 2 && (ok[0], ok[1]) >= (3, 8) {
                found.push((path, version));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use magi_core::magic::InputSpec;

    use super::*;

    /// Downloads ONNX Runtime for graphics cards and runs a model with it on
    /// the CPU and on the first graphics card. With `MAGI_TEST_MODEL` set to
    /// an image classifier, also compares the engines' speed on it.
    #[test]
    #[ignore = "downloads about 55 MB"]
    fn onnx_runtime_downloads_and_runs_models_on_the_cpu_and_a_graphics_card() {
        let dir = std::env::temp_dir().join(format!("magi-components-{}", std::process::id()));
        let components = Components::new(&dir, &dir);
        let message = components.install(Component::OnnxRuntimeGpu, None, &|_| {}).unwrap();
        assert!(message.contains("installed"));
        let (library, gpu) = components.onnxruntime().unwrap();
        assert!(gpu);
        if let Some(directml) = components.directml() {
            assert!(magi_platform::gpu::preload(&directml));
        }
        magi_ml::use_onnxruntime(&library, gpu).unwrap();

        let onnx = include_bytes!("../../magi-engine/tests/data/channel_means.onnx");
        let spec = InputSpec { width: 4, height: 4, mean: [0.0; 3], std: [1.0; 3], ..InputSpec::default() };
        let green: Vec<u8> = (0..64).flat_map(|_| [20u8, 230, 20, 255]).collect();
        for engine in
            [magi_ml::Engine::BuiltIn, magi_ml::Engine::OnnxRuntimeCpu, magi_ml::Engine::OnnxRuntimeGpu { index: 0 }]
        {
            let mut model = magi_ml::Model::load_on(onnx, &spec, engine).unwrap();
            let p = model.classify(&green, 8, 8).unwrap();
            assert!(p[1] > p[0] && p[1] > p[2], "{engine:?}: {p:?}");
        }

        if let Some(path) = std::env::var_os("MAGI_TEST_MODEL") {
            let onnx = std::fs::read(path).unwrap();
            let spec = magi_ml::inspect(&onnx).unwrap().suggested(InputSpec::default());
            let picture: Vec<u8> =
                (0..320 * 320).flat_map(|i| [(i % 251) as u8, (i % 13) as u8 * 19, 90, 255]).collect();
            for engine in [
                magi_ml::Engine::BuiltIn,
                magi_ml::Engine::OnnxRuntimeCpu,
                magi_ml::Engine::OnnxRuntimeGpu { index: 0 },
            ] {
                let mut model = magi_ml::Model::load_on(&onnx, &spec, engine).unwrap();
                let first = model.classify(&picture, 320, 320).unwrap();
                let start = std::time::Instant::now();
                for _ in 0..100 {
                    model.classify(&picture, 320, 320).unwrap();
                }
                let ms = start.elapsed().as_secs_f64() * 10.0;
                println!("{}: {ms:.2} ms per picture, first answers {:?}", engine.label(), &first[..3]);
            }
        }
        assert!(components.uninstall(Component::OnnxRuntimeGpu, None).unwrap().contains("restarts"), "in use");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
