use std::{
    env, fs,
    path::{Path, PathBuf},
};

use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let task = env::args()
        .nth(1)
        .ok_or(anyhow::anyhow!("No sub command"))?;
    match task.as_str() {
        "dist" => dist()?,
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

    println!("Dist available at {}", dist_dir.display());

    Ok(())
}

fn dist_binary() -> Result<(), anyhow::Error> {
    let project_dir = project_root();
    let dist_dir = dist_dir();

    println!("project_dir = {}", project_dir.display());

    let sh = Shell::new()?;
    cmd!(sh, "cargo build --release --locked").run()?;

    let dst = project_root().join("target/release/client");

    fs::copy(&dst, dist_dir.join("client"))?;

    cmd!(sh, "cp -R {project_dir}/wwwroot {dist_dir}").run()?;
    cmd!(sh, "mkdir -p {dist_dir}/website").run()?;
    cmd!(
        sh,
        "cp -R {project_dir}/website/templates {dist_dir}/website"
    )
    .run()?;

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

    cmd!(sh, "mkdir -p wwwroot").run()?;

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
