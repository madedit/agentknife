//! JS libraries a generated app may add (`add_library`), vendored into the
//! knowledge pack (AGENTS.md §2 "JS libraries for generated apps").
//!
//! `cargo xtask vendor-libs` takes each library's files from its npm tarball
//! (checked against the registry's sha512 integrity) and writes `SHA256SUMS`.
//! `cargo xtask ci` verifies the hashes, that versions.toml, pack.toml and the
//! files agree, and that every module a vendored file imports is vendored too.
//! Nothing is downloaded in CI.

use std::io::Read as _;
use std::path::Path;

use sha2::{Digest, Sha512};

use crate::Result;
use crate::vendor::{SUMS_FILE, list_files, sha256_hex, verify_dir};
use crate::versions::Versions;

const PACK_DIR: &str = "knowledge/tauri-leptos";

struct Library {
    /// npm package, also the key in versions.toml `[vendor.libraries]` and the
    /// `[[library]]` id in pack.toml.
    package: &'static str,
    /// Folder under the pack.
    dir: &'static str,
    /// (path in the tarball, path in `dir`).
    files: &'static [(&'static str, &'static str)],
    /// A file and the text it must contain for the pinned version (`{version}`,
    /// `{minor}` are filled in), so a stale copy is caught.
    version_mark: (&'static str, &'static str),
}

const LIBRARIES: &[Library] = &[Library {
    package: "three",
    dir: "libraries/three",
    files: &[
        ("package/build/three.module.js", "three.module.js"),
        ("package/build/three.core.js", "three.core.js"),
        (
            "package/examples/jsm/controls/OrbitControls.js",
            "addons/controls/OrbitControls.js",
        ),
        // Model loaders (P1-KNOW-002) and what they import.
        (
            "package/examples/jsm/loaders/GLTFLoader.js",
            "addons/loaders/GLTFLoader.js",
        ),
        (
            "package/examples/jsm/loaders/OBJLoader.js",
            "addons/loaders/OBJLoader.js",
        ),
        (
            "package/examples/jsm/loaders/MTLLoader.js",
            "addons/loaders/MTLLoader.js",
        ),
        (
            "package/examples/jsm/loaders/FBXLoader.js",
            "addons/loaders/FBXLoader.js",
        ),
        (
            "package/examples/jsm/utils/BufferGeometryUtils.js",
            "addons/utils/BufferGeometryUtils.js",
        ),
        (
            "package/examples/jsm/utils/SkeletonUtils.js",
            "addons/utils/SkeletonUtils.js",
        ),
        (
            "package/examples/jsm/libs/fflate.module.js",
            "addons/libs/fflate.module.js",
        ),
        (
            "package/examples/jsm/curves/NURBSCurve.js",
            "addons/curves/NURBSCurve.js",
        ),
        (
            "package/examples/jsm/curves/NURBSUtils.js",
            "addons/curves/NURBSUtils.js",
        ),
        ("package/LICENSE", "LICENSE"),
    ],
    version_mark: ("three.core.js", "const REVISION = '{minor}';"),
}];

fn fetch(url: &str) -> Result<Vec<u8>> {
    Ok(ureq::get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()?)
}

/// A large file in 1 MB ranges, each tried a few times: some networks cut
/// long downloads off (the integrity check still covers the whole file).
fn fetch_in_parts(url: &str) -> Result<Vec<u8>> {
    const PART: usize = 1 << 20;
    let mut out = Vec::new();
    loop {
        let range = format!("bytes={}-{}", out.len(), out.len() + PART - 1);
        let mut last = None;
        let mut part = None;
        for _ in 0..5 {
            match ureq::get(url).header("Range", &range).call() {
                Ok(mut r) => {
                    let total = r
                        .headers()
                        .get("Content-Range")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.rsplit('/').next())
                        .and_then(|v| v.parse::<usize>().ok());
                    match r
                        .body_mut()
                        .with_config()
                        .limit(2 * PART as u64)
                        .read_to_vec()
                    {
                        Ok(bytes) => {
                            part = Some((bytes, total));
                            break;
                        }
                        Err(e) => last = Some(e.to_string()),
                    }
                }
                Err(e) => last = Some(e.to_string()),
            }
        }
        let Some((bytes, total)) = part else {
            return Err(
                format!("{url} at byte {}: {}", out.len(), last.unwrap_or_default()).into(),
            );
        };
        let Some(total) = total else {
            // The server sent the whole file instead of a range.
            return Ok(bytes);
        };
        out.extend_from_slice(&bytes);
        if bytes.is_empty() || out.len() >= total {
            return Ok(out);
        }
    }
}

/// The modules `source` imports (`from '…'`), as written.
fn imports(source: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?m)^\s*(?:import|export)\b[^;]*?\bfrom\s*['"]([^'"]+)['"]"#)
        .unwrap_or_else(|e| panic!("{e}"));
    re.captures_iter(source).map(|c| c[1].to_owned()).collect()
}

/// Imports in `dir`'s JS files that are neither `three` nor a vendored file.
fn missing_imports(dir: &Path) -> Result<Vec<String>> {
    let files = list_files(dir, dir)?;
    let mut missing = Vec::new();
    for rel in files.iter().filter(|f| f.ends_with(".js")) {
        let text = std::fs::read_to_string(dir.join(rel))?;
        let base = Path::new(rel).parent().unwrap_or(Path::new(""));
        for module in imports(&text) {
            if module == "three" {
                continue;
            }
            let mut parts: Vec<String> = base
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            for p in module.split('/') {
                match p {
                    "." => {}
                    ".." => {
                        parts.pop();
                    }
                    _ => parts.push(p.to_owned()),
                }
            }
            let target = parts.join("/");
            if !module.starts_with('.') || !files.contains(&target) {
                missing.push(format!("{rel} imports {module}"));
            }
        }
    }
    Ok(missing)
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ABC[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn mark(lib: &Library, version: &str) -> String {
    let minor = version.split('.').nth(1).unwrap_or_default();
    lib.version_mark
        .1
        .replace("{version}", version)
        .replace("{minor}", minor)
}

pub fn vendor(root: &Path) -> Result<()> {
    let pinned = Versions::load(root)?.vendor.libraries;
    for lib in LIBRARIES {
        let version = pinned
            .get(lib.package)
            .ok_or_else(|| format!("versions.toml [vendor.libraries] has no {}", lib.package))?;
        let meta: serde_json::Value = serde_json::from_slice(&fetch(&format!(
            "https://registry.npmjs.org/{}/{version}",
            lib.package
        ))?)?;
        let url = meta["dist"]["tarball"].as_str().ok_or("no dist.tarball")?;
        let integrity = meta["dist"]["integrity"]
            .as_str()
            .ok_or("no dist.integrity")?;
        println!("==> {} {version}: {url}", lib.package);
        let tgz = fetch_in_parts(url)?;
        let actual = format!("sha512-{}", base64(&Sha512::digest(&tgz)));
        if actual != integrity {
            return Err(format!("{url}: integrity {actual} != registry {integrity}").into());
        }
        let dir = root.join(PACK_DIR).join(lib.dir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&tgz[..]));
        let mut found = 0;
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name = entry.path()?.to_string_lossy().replace('\\', "/");
            if let Some((_, to)) = lib.files.iter().find(|(from, _)| *from == name) {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                let out = dir.join(to);
                std::fs::create_dir_all(out.parent().ok_or("bad path")?)?;
                std::fs::write(out, bytes)?;
                found += 1;
            }
        }
        if found != lib.files.len() {
            return Err(format!("{url}: found {found} of {} files", lib.files.len()).into());
        }
        let mut sums = String::new();
        for rel in list_files(&dir, &dir)? {
            sums.push_str(&format!(
                "{}  {rel}\n",
                sha256_hex(&std::fs::read(dir.join(&rel))?)
            ));
        }
        std::fs::write(dir.join(SUMS_FILE), sums)?;
        println!("vendored {} into {}", lib.package, dir.display());
    }
    println!("now set the [[library]] version in {PACK_DIR}/pack.toml to match");
    Ok(())
}

/// pack.toml's `[[library]]` id -> version.
fn pack_versions(root: &Path) -> Result<Vec<(String, String)>> {
    let pack: toml::Table = toml::from_str(&std::fs::read_to_string(
        root.join(PACK_DIR).join("pack.toml"),
    )?)?;
    Ok(pack
        .get("library")
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
        .filter_map(|l| {
            Some((
                l.get("id")?.as_str()?.to_owned(),
                l.get("version")?.as_str()?.to_owned(),
            ))
        })
        .collect())
}

pub fn check(root: &Path) -> Result<()> {
    let pinned = Versions::load(root)?.vendor.libraries;
    let in_pack = pack_versions(root)?;
    let mut errors = Vec::new();
    for lib in LIBRARIES {
        let Some(version) = pinned.get(lib.package) else {
            errors.push(format!(
                "versions.toml [vendor.libraries]: {} missing",
                lib.package
            ));
            continue;
        };
        let dir = root.join(PACK_DIR).join(lib.dir);
        errors.extend(verify_dir(&dir)?);
        errors.extend(
            missing_imports(&dir)?
                .into_iter()
                .map(|m| format!("{}: {m}, which is not vendored", lib.dir)),
        );
        let (file, _) = lib.version_mark;
        let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
        if !text.contains(&mark(lib, version)) {
            errors.push(format!(
                "{}/{file}: not version {version}; run `cargo xtask vendor-libs`",
                lib.dir
            ));
        }
        match in_pack.iter().find(|(id, _)| id == lib.package) {
            Some((_, v)) if v == version => {}
            Some((_, v)) => errors.push(format!(
                "pack.toml library {}: version {v}, versions.toml says {version}",
                lib.package
            )),
            None => errors.push(format!("pack.toml: no [[library]] {}", lib.package)),
        }
    }
    for (id, _) in &in_pack {
        if !LIBRARIES.iter().any(|l| l.package == id) {
            errors.push(format!(
                "pack.toml library {id}: not vendored by xtask/src/libs.rs"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn imports_must_be_vendored() {
        let dir = std::env::temp_dir().join(format!("ak-libs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |rel: &str, text: &str| {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap_or(&dir)).unwrap_or_else(|e| panic!("{e}"));
            std::fs::write(p, text).unwrap_or_else(|e| panic!("{e}"));
        };
        write(
            "addons/loaders/A.js",
            "import {\n  Mesh,\n} from 'three';\nimport { b } from '../utils/B.js';\nimport { c } from '../libs/C.js';\n",
        );
        write("addons/utils/B.js", "export { x } from \"./D.js\";\n");
        write(
            "addons/utils/D.js",
            "// @three_import import { A } from 'three/addons/A.js';\n",
        );
        let missing = missing_imports(&dir).unwrap_or_else(|e| panic!("{e}"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(missing, ["addons/loaders/A.js imports ../libs/C.js"]);
    }

    #[test]
    fn version_mark_uses_the_minor_number() {
        assert_eq!(mark(&LIBRARIES[0], "0.186.1"), "const REVISION = '186';");
    }
}
