// Copyright (c) 2019 - 2020 ESRLabs
//
//   Licensed under the Apache License, Version 2.0 (the "License");
//   you may not use this file except in compliance with the License.
//   You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//   Unless required by applicable law or agreed to in writing, software
//   distributed under the License is distributed on an "AS IS" BASIS,
//   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//   See the License for the specific language governing permissions and
//   limitations under the License.

use anyhow::{anyhow, Context, Error, Result};
use async_std::{path::Path, task};
use serde::{
    de::{Deserializer, Visitor},
    ser::Serializer,
    Deserialize, Serialize,
};
use std::{collections::HashMap, fmt, fs::File, str::FromStr};

#[derive(Clone, PartialOrd, Hash, Eq, PartialEq)]
pub struct Version(semver::Version);

impl Version {
    #[allow(dead_code)]
    pub fn parse(s: &str) -> Result<Version> {
        Ok(Version(semver::Version::parse(s)?))
    }
}

impl Default for Version {
    fn default() -> Version {
        Version(semver::Version::new(0, 0, 0))
    }
}

impl Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct VersionVisitor;

        impl<'de> Visitor<'de> for VersionVisitor {
            type Value = Version;
            fn visit_str<E>(self, str_data: &str) -> Result<Version, E>
            where
                E: serde::de::Error,
            {
                semver::Version::parse(str_data).map(Version).map_err(|_| {
                    serde::de::Error::invalid_value(::serde::de::Unexpected::Str(str_data), &self)
                })
            }

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str("string v0.0.0")
            }
        }

        deserializer.deserialize_str(VersionVisitor)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum OnExit {
    /// Container is restarted n number and not started anymore after n exits
    #[serde(rename = "restart")]
    Restart(u32),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CGroupMem {
    /// Limit im bytes
    pub limit: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CGroupCpu {
    /// CPU shares
    pub shares: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CGroups {
    pub mem: Option<CGroupMem>,
    pub cpu: Option<CGroupCpu>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum LogBuffer {
    #[serde(rename(serialize = "main", deserialize = "main"))]
    Main,
    #[serde(rename(serialize = "custom", deserialize = "custom"))]
    Custom(u8),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Log {
    pub tag: Option<String>,
    pub buffer: Option<LogBuffer>,
}

#[derive(Clone, Default, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Name of container
    pub name: String,
    /// Container version
    pub version: Version,
    /// Target arch
    pub arch: String,
    /// Path to init
    pub init: std::path::PathBuf,
    /// Additional arguments for the application invocation
    pub args: Option<Vec<String>>,
    /// Environment passed to container
    pub env: Option<Vec<(String, String)>>,
    /// Autostart this container upon north startup
    pub autostart: Option<bool>,
    /// Action on application exit
    pub on_exit: Option<OnExit>,
    /// CGroup config
    pub cgroups: Option<CGroups>,
    /// Seccomp configuration
    pub seccomp: Option<HashMap<String, String>>,
    /// Number of instances to mount of this container
    /// The name get's extended with the instance id.
    pub instances: Option<u32>,
    /// Log priority of stdout
    pub log: Option<Log>,
}

impl Manifest {
    pub async fn from_path(f: &Path) -> Result<Manifest> {
        let f = f.to_owned();
        task::spawn_blocking(move || {
            let file = File::open(&f)?;
            let manifest: Manifest = serde_yaml::from_reader(file)
                .with_context(|| format!("Failed to parse {}", f.display()))?;

            if let Some(OnExit::Restart(n)) = manifest.on_exit {
                if n == 0 {
                    return Err(anyhow!("Invalid on_exit value in {}", f.display()));
                }
            }
            Ok(manifest)
        })
        .await
    }
}

impl FromStr for Manifest {
    type Err = Error;
    fn from_str(s: &str) -> std::result::Result<Manifest, Error> {
        serde_yaml::from_str(s).context("Failed to parse manifest")
    }
}

#[async_std::test]
async fn parse() -> Result<()> {
    use async_std::path::PathBuf;
    use std::{fs::File, io::Write};

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path();

    let m = "
name: hello
version: 0.0.0
arch: aarch64-linux-android
init: /binary
args: [one, two]
env: [[LD_LIBRARY_PATH, /lib]]
autostart: true
on_exit:
    restart: 3
cgroups:
  mem:
    limit: 30
  cpu:
    shares: 100
seccomp:
    fork: 1
    waitpid: 1
log:
    tag: test
    buffer:
        custom: 8
";

    let mut file = File::create(path)?;
    file.write_all(m.as_bytes())?;
    drop(file);

    let manifest = Manifest::from_path(&PathBuf::from(path)).await?;

    assert_eq!(manifest.init, std::path::PathBuf::from("/binary"));
    assert_eq!(manifest.name, "hello");
    let args = manifest.args.ok_or_else(|| anyhow!("Missing args"))?;
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], "one");
    assert_eq!(args[1], "two");
    assert!(manifest.autostart.unwrap());
    assert_eq!(manifest.on_exit, Some(OnExit::Restart(3)));
    let env = manifest.env.ok_or_else(|| anyhow!("Missing env"))?;
    assert_eq!(env[0], ("LD_LIBRARY_PATH".into(), "/lib".into()));
    assert_eq!(
        manifest.cgroups,
        Some(CGroups {
            mem: Some(CGroupMem { limit: 30 }),
            cpu: Some(CGroupCpu { shares: 100 }),
        })
    );

    let mut seccomp = HashMap::new();
    seccomp.insert("fork".to_string(), "1".to_string());
    seccomp.insert("waitpid".to_string(), "1".to_string());
    assert_eq!(manifest.seccomp, Some(seccomp));
    assert_eq!(manifest.log.as_ref().unwrap().tag.as_ref().unwrap(), "test");
    assert_eq!(manifest.log.unwrap().buffer.unwrap(), LogBuffer::Custom(8));

    Ok(())
}

#[async_std::test]
async fn parse_invalid_on_exit() -> std::io::Result<()> {
    use async_std::path::PathBuf;
    use std::{fs::File, io::Write};

    let file = tempfile::NamedTempFile::new()?;
    let path = file.path();

    let m = "
name: hello
version: 0.0.0
arch: aarch64-linux-android
init: /binary
args: [one, two]
env: [[LD_LIBRARY_PATH, /lib]]
on_exit:
    Restart: 0
";

    let mut file = File::create(path)?;
    file.write_all(m.as_bytes())?;
    drop(file);

    let manifest = Manifest::from_path(&PathBuf::from(path)).await;
    assert!(manifest.is_err());
    Ok(())
}

#[test]
fn version() -> Result<()> {
    let v1 = Version::parse("1.0.0")?;
    let v2 = Version::parse("2.0.0")?;
    let v3 = Version::parse("3.0.0")?;
    assert!(v2 > v1);
    assert!(v2 < v3);
    let v1_1 = Version::parse("1.1.0")?;
    assert!(v1_1 > v1);
    let v1_1_1 = Version::parse("1.1.1")?;
    assert!(v1_1_1 > v1_1);
    Ok(())
}
