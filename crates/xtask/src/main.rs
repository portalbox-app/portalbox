use std::{
    env,
    fs::{self},
    path::{Path, PathBuf},
};

use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let task = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("No sub command"))?;
    match task.as_str() {
        "dist" => dist()?,
        "release" => release()?,
        "unrelease" => unrelease()?,
        "clean_web" => clean_web()?,
        "build_web" => build_web()?,
        _ => return Err(anyhow::anyhow!("Unexpected sub command")),
    }

    Ok(())
}

fn dist() -> Result<(), anyhow::Error> {
    let dist_dir = dist_dir();

    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    build_web()?;
    dist_binary()?;
    println!("Done building binaries");
    let output_name = {
        let version = version()?;
        let platform_arch = get_out_platform_arch();
        format!("portalbox-{version}-{platform_arch}")
    };
    let sh = Shell::new()?;
    cmd!(sh, "tar -czf {output_name}.tar.gz -C target/dist .").run()?;

    cfg_if::cfg_if! {
        if #[cfg(target_os = "windows")] {
            cmd!(sh, "powershell Get-FileHash -Path {output_name}.tar.gz").run()?;
        } else {
            cmd!(sh, "shasum -a 256 {output_name}.tar.gz").run()?;
        }
    };

    println!("Dist available at {}", dist_dir.display());

    Ok(())
}

fn dist_binary() -> Result<(), anyhow::Error> {
    let project_dir = project_root();
    let dist_dir = dist_dir();

    println!("project_dir = {}", project_dir.display());

    let sh = Shell::new()?;
    cmd!(sh, "cargo build --release --locked").run()?;

    let binary_filename;
    cfg_if::cfg_if! {
        if #[cfg(target_os = "windows")] {
            binary_filename = "portalbox.exe";
        } else {
            binary_filename = "portalbox";
        }
    };

    let dst = project_root().join("target/release").join(binary_filename);

    fs::copy(&dst, dist_dir.join(binary_filename))?;

    cmd!(sh, "cp -R {project_dir}/wwwroot {dist_dir}").run()?;
    cmd!(sh, "mkdir -p {dist_dir}/website").run()?;
    cmd!(
        sh,
        "cp -R {project_dir}/website/templates {dist_dir}/website"
    )
    .run()?;

    Ok(())
}

fn release() -> Result<(), anyhow::Error> {
    let version = version()?;
    println!("Making a release version = {version}");

    let project_dir = project_root();

    let sh = Shell::new()?;
    sh.change_dir(&project_dir);
    let tag_msg = format!("Version {version}");

    cmd!(sh, "git tag -a v{version} -m {tag_msg}").run()?;
    cmd!(sh, "git push --tags").run()?;

    Ok(())
}

fn unrelease() -> Result<(), anyhow::Error> {
    let version = version()?;
    println!("Removing a release version = {version}");

    let project_dir = project_root();

    let sh = Shell::new()?;
    sh.change_dir(&project_dir);

    cmd!(sh, "git tag -d v{version}").run()?;
    cmd!(sh, "git push --delete origin v{version}").run()?;

    Ok(())
}

fn clean_web() -> Result<(), anyhow::Error> {
    let project_dir = project_root();
    let wwwroot_dir = project_dir.join("wwwroot");

    let _ = fs::remove_dir_all(&wwwroot_dir);

    Ok(())
}

fn build_web() -> Result<(), anyhow::Error> {
    clean_web()?;

    let project_dir = project_root();

    let sh = Shell::new()?;
    sh.change_dir(&project_dir);

    sh.create_dir("wwwroot")?;

    {
        let dir = project_dir.join("website/static");
        let _webdir = sh.push_dir(dir);
        cfg_if::cfg_if! {
            if #[cfg(target_os = "windows")] {
                cmd!(sh, "powershell npm install").run()?;
            } else {
                cmd!(sh, "npm install").run()?;
            }
        }
    }

    cmd!(sh, "cp -r {project_dir}/website/static/ wwwroot").run()?;

    Ok(())
}

fn version() -> Result<String, anyhow::Error> {
    let project_dir = project_root();
    let cargo_toml_path = project_dir.join("crates/client/Cargo.toml");
    let cargo_toml_content = std::fs::read_to_string(cargo_toml_path)?;
    let value: toml::Value = toml::from_str(&cargo_toml_content)?;

    let version = value["package"]["version"].clone();
    let ret = version
        .as_str()
        .map(|val| val.to_string())
        .ok_or(anyhow::anyhow!("No version found"))?;

    Ok(ret)
}

fn get_build_arch() -> String {
    "x64".into()
}

fn get_out_platform_arch() -> String {
    let os = std::env::consts::OS;
    let arch = get_build_arch();

    format!("{os}-{arch}")
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn dist_dir() -> PathBuf {
    project_root().join("target/dist")
}
