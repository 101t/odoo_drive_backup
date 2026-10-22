//! Runs the server under the Windows Service Control Manager.
//!
//! `packaging/windows/install.ps1` registers the service with the command line
//! `odoo-backup.exe windows-service <env file>`. Settings come from that file (the same `ODB_*` variables as on
//! Linux), and logs go to `<data dir>\logs` because a service has no console.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tokio_util::sync::CancellationToken;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::{define_windows_service, service_dispatcher};

const SERVICE_NAME: &str = "OdooBackup";

static STATUS: OnceLock<ServiceStatusHandle> = OnceLock::new();

pub fn run(env_file: Option<PathBuf>) -> anyhow::Result<()> {
    let loaded = match &env_file {
        Some(path) => dotenvy::from_path(path).with_context(|| format!("reading {}", path.display())),
        None => Ok(()),
    };
    if std::env::var_os("ODB_DATA_DIR").is_none() {
        let program_data =
            std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        // SAFETY: no other thread exists yet (the dispatcher and the runtime start later).
        unsafe { std::env::set_var("ODB_DATA_DIR", program_data.join("OdooBackup")) };
    }
    init_logging(Path::new(&std::env::var_os("ODB_DATA_DIR").unwrap_or_default()))?;
    if let Err(error) = loaded {
        tracing::error!("{error:#}");
        return Err(error);
    }

    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("connecting to the Windows service manager (this command only runs as a service)")
}

fn init_logging(data_dir: &Path) -> anyhow::Result<()> {
    let logs = data_dir.join("logs");
    std::fs::create_dir_all(&logs).with_context(|| format!("creating log directory {}", logs.display()))?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("odoo-backup")
        .filename_suffix("log")
        .max_log_files(14)
        .build(&logs)
        .with_context(|| format!("opening the log file in {}", logs.display()))?;
    tracing_subscriber::fmt().with_env_filter(crate::log_filter()).with_ansi(false).with_writer(appender).init();
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    match run_service() {
        Ok(()) => set_status(ServiceState::Stopped, ServiceExitCode::Win32(0), Duration::ZERO),
        Err(error) => {
            tracing::error!("{error:#}");
            set_status(ServiceState::Stopped, ServiceExitCode::ServiceSpecific(1), Duration::ZERO);
        }
    }
}

fn run_service() -> anyhow::Result<()> {
    let stop = CancellationToken::new();
    let on_stop = stop.clone();
    let handle = service_control_handler::register(SERVICE_NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            // Running backups get up to 20 s to record their state (see Engine::shutdown).
            set_status(ServiceState::StopPending, ServiceExitCode::Win32(0), Duration::from_secs(30));
            on_stop.cancel();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let _ = STATUS.set(handle);
    set_status(ServiceState::Running, ServiceExitCode::Win32(0), Duration::ZERO);

    let config = crate::Cli::try_parse_from(["odoo-backup"])?.config;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(crate::serve(config, stop.cancelled_owned()))
}

fn set_status(state: ServiceState, exit_code: ServiceExitCode, wait_hint: Duration) {
    let Some(handle) = STATUS.get() else { return };
    let controls_accepted = match state {
        ServiceState::Running => ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        _ => ServiceControlAccept::empty(),
    };
    let status = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted,
        exit_code,
        checkpoint: 0,
        wait_hint,
        process_id: None,
    };
    if let Err(error) = handle.set_service_status(status) {
        tracing::warn!("cannot report the service status: {error}");
    }
}
