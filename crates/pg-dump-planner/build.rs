use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn lock(path: &Path) -> fs::File {
    let file = fs::File::create(path).expect("open native planner cache lock");
    file.lock().expect("lock native planner cache");
    file
}

fn main() {
    println!("cargo:rerun-if-changed=native");
    for input in [
        "prepare-native.sh",
        "verify_upstream.sh",
        "upstream-blobs.txt",
        "upstream-revision",
    ] {
        println!("cargo:rerun-if-changed={input}");
    }
    for var in [
        "PG_DUMP_SOURCE",
        "PG_DUMP_BUILD",
        "CPPFLAGS",
        "LDFLAGS",
        "SDKROOT",
        "MACOSX_DEPLOYMENT_TARGET",
        "PQ_LIB_DIR",
        "OBJCOPY",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=planner");
    let target = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../target");
    let source = env::var_os("PG_DUMP_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| target.join("pg-dump-source"));
    let build = env::var_os("PG_DUMP_BUILD")
        .map(PathBuf::from)
        .unwrap_or_else(|| target.join("pg-dump-build"));
    // Debug, release and separate Cargo target directories share this native tree.
    fs::create_dir_all(&build).expect("create native planner build directory");
    let _build_lock = lock(&build.join(".pgex-build.lock"));
    // Concurrent builds may share their checkout but use different build trees.
    let fetch_lock = if source.is_dir() {
        None
    } else {
        fs::create_dir_all(source.parent().unwrap()).expect("create native source cache directory");
        Some(lock(&source.with_added_extension("fetch.lock")))
    };
    assert!(
        Command::new("sh")
            .arg("prepare-native.sh")
            .arg(&source)
            .arg(&build)
            // PostgreSQL make treats PROFILE as extra C compiler flags.
            .env_remove("PROFILE")
            .status()
            .expect("run PostgreSQL native build")
            .success(),
        "building the pinned PostgreSQL planner dependencies failed"
    );
    drop(fetch_lock);
    // Only the vendored sources, pin, and build rules are Cargo inputs.
    // Watching make's outputs here would invalidate this build as it runs.
    let config_path = build.join("src/include/pg_config.h");
    let config =
        std::fs::read_to_string(config_path).expect("read generated PostgreSQL configuration");
    assert!(
        config.lines().any(|l| l == "#define PG_VERSION_NUM 180007"),
        "planner requires PG18.7 build headers"
    );
    let dump = PathBuf::from("native/upstream/src/bin/pg_dump");
    let mut compiler = cc::Build::new();
    compiler
        .include(&dump)
        .include(build.join("src/include"))
        .include(source.join("src/include"))
        .include(source.join("src/interfaces/libpq"))
        .define("FRONTEND", None)
        .warnings(false)
        .opt_level(2)
        .flag("-ffunction-sections")
        .flag("-fdata-sections")
        .file("planner/child_plan.c")
        .file("planner/restore_pass.c");
    for name in [
        "common.c",
        "pg_dump_sort.c",
        "pg_backup_db.c",
        "pg_backup_utils.c",
        "dumputils.c",
    ] {
        compiler.file(dump.join(name));
    }
    // pg_query embeds backend routines with some of the same symbol names
    // (notably palloc). Bind the frontend helpers privately as one object.
    let objects = compiler.compile_intermediates();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let merged = out.join("pg_dump_planner.o");
    let mut link = compiler.get_compiler().to_command();
    let darwin = env::var("CARGO_CFG_TARGET_VENDOR").unwrap() == "apple";
    link.args(["-nostdlib", "-r"]);
    if darwin {
        link.arg("-Wl,-exported_symbol,_pgex_child_plan");
    } else {
        link.arg("-no-pie");
    }
    link.args(objects);
    for (dir, lib) in [
        ("fe_utils", "pgfeutils"),
        ("common", "pgcommon"),
        ("port", "pgport"),
    ] {
        let archive = build.join("src").join(dir).join(format!("lib{lib}.a"));
        link.arg(archive);
    }
    assert!(
        link.arg("-o")
            .arg(&merged)
            .status()
            .expect("bind planner objects")
            .success()
    );
    if !darwin {
        let objcopy = env::var_os("OBJCOPY").unwrap_or_else(|| "objcopy".into());
        assert!(
            Command::new(objcopy)
                .arg("--keep-global-symbol=pgex_child_plan")
                .arg(&merged)
                .status()
                .expect("localize planner symbols")
                .success()
        );
    }
    cc::Build::new()
        .object(merged)
        .compile("pgex_planner_child");
}
