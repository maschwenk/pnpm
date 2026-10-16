use super::{DedicatedProjectRuns, STARTED_PER_SLOT};
use crate::{State, cli_args::pipelines::DedicatedProjects};
use futures_util::future::OptionFuture;
use indexmap::IndexMap;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// Every project of the chain waits, with its install in memory, for the
/// project before it, as a project deep in a workspace dependency graph does.
#[tokio::test]
async fn a_chain_of_waiting_projects_keeps_a_bounded_number_of_installs_started() {
    let workspace = tempfile::tempdir().expect("create workspace dir");
    let project_dirs: Vec<PathBuf> = (0..20)
        .map(|index| {
            let project_dir = workspace
                .path()
                .join(format!("p{index}"));
            fs::create_dir_all(&project_dir).expect("create project dir");
            fs::write(project_dir.join("package.json"), format!(r#"{{ "name": "p{index}" }}"#))
                .expect("write package.json");
            project_dir
        })
        .collect();
    let dependencies: IndexMap<PathBuf, Vec<PathBuf>> = project_dirs
        .iter()
        .enumerate()
        .map(|(index, project_dir)| {
            (
                project_dir.clone(),
                project_dirs[..index]
                    .last()
                    .cloned()
                    .into_iter()
                    .collect(),
            )
        })
        .collect();
    let config = Config { workspace_concurrency: 1, ..Config::default() };
    let runs = DedicatedProjectRuns {
        config: &config,
        projects: DedicatedProjects {
            early_starts: project_dirs.iter().cloned().collect(),
            dependencies,
            names: HashMap::new(),
            covers_workspace: true,
            injected_source_dirs: HashSet::new(),
        },
        require_lockfile: false,
        http_client: Some(Arc::new(ThrottledClient::default())),
        prune_excludes: false,
        sync_injected_deps: false,
        pipelined: true,
    };
    let started = Arc::new(AtomicUsize::new(0));
    let most_started = Arc::new(AtomicUsize::new(0));

    runs.run_projects_pipelined(|state: State| {
        let (started, most_started) = (Arc::clone(&started), Arc::clone(&most_started));
        async move {
            let now_started = started.fetch_add(1, Ordering::SeqCst) + 1;
            most_started.fetch_max(now_started, Ordering::SeqCst);
            state.dedicated
                .and_then(|dedicated| dedicated.dependencies_installed)
                .pipe(OptionFuture::from)
                .await;
            started.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .expect("install every project");

    assert_eq!(most_started.load(Ordering::SeqCst), STARTED_PER_SLOT);
}
