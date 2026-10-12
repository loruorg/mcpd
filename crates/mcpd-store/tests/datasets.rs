//! Datasets and experiments, through the `Store` trait.
//!
//! Every assertion in this file runs against each backend the build can reach:
//! SQLite always, and Postgres too when `MCPD_TEST_POSTGRES_URL` names a server,
//! the same gate `postgres.rs` uses. The two backends were written from
//! different statements for these six methods, so a behaviour proved on one of
//! them is not proved.
//!
//! Ownership is proved on a third. `dataset_project` decides which tenant may
//! read a dataset, so it is checked against ClickHouse as well when
//! `MCPD_TEST_CLICKHOUSE_URL` names a server, and it is the only thing here
//! that is: see [`owner_backends`] for why the rest cannot be yet.

use mcpd_core::{Correlation, Cost, Dataset, DatasetItem, Experiment, ExperimentStatus};
use mcpd_store::{
    ClickHouseConfig, ClickHouseStore, PostgresConfig, PostgresStore, SqliteStore, Store,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

/// A backend under test, with the name to name in a failure. Without it a
/// failing assertion says nothing about which store is wrong.
type Backend = (&'static str, Box<dyn Store>);

/// Every backend this build can reach, each already migrated.
///
/// SQLite gets a fresh file per call, so tests cannot see each other's rows.
/// Postgres is shared and durable, which is what `unique` is for.
async fn backends() -> Vec<Backend> {
    require_live();
    let mut out: Vec<Backend> = Vec::new();

    let dir = std::env::temp_dir().join(format!("mcpd-datasets-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let sqlite =
        SqliteStore::open(dir.join(format!("{}.db", unique("store"))), 2).expect("open sqlite");
    sqlite.migrate().await.expect("migrate sqlite");
    out.push(("sqlite", Box::new(sqlite)));

    if let Ok(url) = std::env::var("MCPD_TEST_POSTGRES_URL") {
        let pg = PostgresStore::new(PostgresConfig { url, ..Default::default() })
            .expect("build the postgres store");
        pg.migrate().await.expect("migrate postgres");
        out.push(("postgres", Box::new(pg)));
    }
    out
}

/// The ClickHouse store, when `MCPD_TEST_CLICKHOUSE_URL` names a server.
///
/// A database of its own per process, the way `clickhouse.rs` does it, so a run
/// of this file cannot read rows another test binary wrote.
///
/// Guarded like [`backends`], so the ClickHouse-only test below cannot skip
/// unnoticed in a job that set `MCPD_REQUIRE_LIVE` and then reached nothing.
async fn clickhouse_store() -> Option<ClickHouseStore> {
    require_live();
    let url = std::env::var("MCPD_TEST_CLICKHOUSE_URL").ok()?;
    let ch = ClickHouseStore::new(ClickHouseConfig {
        url,
        database: format!("mcpd_datasets_{}", std::process::id()),
        ..Default::default()
    })
    .expect("build the clickhouse store");
    ch.migrate().await.expect("migrate clickhouse");
    Some(ch)
}

/// Every backend that can be asked who owns a dataset.
///
/// Wider than [`backends`] on purpose. ClickHouse cannot join that list yet:
/// it does not recompute `item_count` when cases are added, so
/// `adding_the_same_item_twice_does_not_double_the_item_count` fails against
/// it, and that is a gap in the backend rather than in the test. Ownership does
/// not depend on any of that, and it is the answer that decides who may read a
/// dataset, so it is proved on all three now rather than waiting for the rest.
async fn owner_backends() -> Vec<Backend> {
    let mut out = backends().await;
    if let Some(ch) = clickhouse_store().await {
        out.push(("clickhouse", Box::new(ch)));
    }
    out
}

/// Fail rather than skip when a job that started a service loses the variable
/// pointing at it.
///
/// SQLite is always here, so this file can never skip outright, and that makes
/// the silence worse rather than better: with no URL the suite still reports
/// every test green, having proved on one backend what it meant to prove on
/// two. `MCPD_REQUIRE_LIVE` turns that into a failure. It is the same guard
/// `postgres.rs` and `clickhouse.rs` carry, and the same one the SDK live
/// suites carry.
///
/// It asks for one live URL rather than naming which. The Postgres job supplies
/// Postgres and the ClickHouse job supplies ClickHouse, and neither has the
/// other, so demanding a particular one would fail a job that was configured
/// correctly.
fn require_live() {
    if std::env::var("MCPD_REQUIRE_LIVE").is_err() {
        return;
    }
    let names = ["MCPD_TEST_POSTGRES_URL", "MCPD_TEST_CLICKHOUSE_URL"];
    assert!(
        names.iter().any(|n| std::env::var(n).is_ok()),
        "MCPD_REQUIRE_LIVE is set but neither MCPD_TEST_POSTGRES_URL nor \
         MCPD_TEST_CLICKHOUSE_URL is, so this suite would have reported green \
         having reached SQLite and nothing else. That is the exact silence this \
         variable exists to break."
    );
}

/// A name no other run of this suite will produce.
///
/// Postgres keeps what earlier runs wrote. A fixed dataset id or project name
/// would make the second run of this file read the first run's rows, so the
/// tests would only pass against a database nobody had used.
fn unique(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Only the project and the environment have a column on a dataset or an
/// experiment. The rest of a `Correlation` is left empty here so the fixture
/// can be compared with what came back, whole.
fn corr(project: &str) -> Correlation {
    Correlation {
        project: project.to_owned(),
        environment: Some("staging".into()),
        service_name: None,
        service_version: None,
        session_id: None,
        thread_id: None,
        user_id: None,
        resource_id: None,
        run_id: None,
        request_id: None,
        experiment_id: None,
        release: None,
    }
}

/// A fixed instant carrying a nanosecond part, so a column that rounded to the
/// second or the millisecond would not survive a comparison.
fn created() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
        + Duration::seconds(1_700_000_000)
        + Duration::nanoseconds(123_456_789)
}

fn dataset(id: &str, project: &str) -> Dataset {
    Dataset {
        dataset_id: id.to_owned(),
        name: "Refunds a person marked wrong".into(),
        description: Some("Captured from production, one case per complaint".into()),
        item_count: 0,
        created_at: created(),
        correlation: corr(project),
    }
}

fn item(dataset_id: &str, n: u32) -> DatasetItem {
    DatasetItem {
        item_id: format!("{dataset_id}-item-{n}"),
        dataset_id: dataset_id.to_owned(),
        input: json!({ "question": format!("where is order {n}") }),
        expected: Some(json!({ "text": "it shipped on Tuesday" })),
        metadata: Some(json!({ "difficulty": "hard" })),
        source_trace_id: Some(format!("{:032x}", 0x00c0_ffee_u64 + u64::from(n))),
    }
}

fn find_dataset(list: &[Dataset], id: &str, backend: &str) -> Dataset {
    list.iter()
        .find(|d| d.dataset_id == id)
        .unwrap_or_else(|| panic!("{backend} did not list dataset {id}"))
        .clone()
}

fn find_experiment(list: &[Experiment], id: &str, backend: &str) -> Experiment {
    list.iter()
        .find(|e| e.experiment_id == id)
        .unwrap_or_else(|| panic!("{backend} did not list experiment {id}"))
        .clone()
}

#[tokio::test]
async fn a_dataset_comes_back_exactly_as_it_went_in() {
    let project = unique("project");
    let id = unique("dataset");
    let want = dataset(&id, &project);

    for (backend, store) in backends().await {
        store.put_dataset(&want).await.expect("put the dataset");

        let listed = store.list_datasets(&project).await.expect("list datasets");
        let got = find_dataset(&listed, &id, backend);
        // Compared whole: the name, the description, the item count, the
        // nanosecond-precise instant and the correlation all have to survive,
        // and nothing may be invented on the way back.
        assert_eq!(got, want, "{backend} changed the dataset");
        assert_eq!(got.created_at, created(), "{backend} lost time precision");
        assert_eq!(got.correlation.project, project, "{backend} lost the project");
        assert_eq!(
            got.correlation.environment.as_deref(),
            Some("staging"),
            "{backend} lost the environment"
        );
    }
}

#[tokio::test]
async fn adding_the_same_item_twice_does_not_double_the_item_count() {
    let project = unique("project");
    let id = unique("dataset");

    for (backend, store) in backends().await {
        store.put_dataset(&dataset(&id, &project)).await.expect("put the dataset");

        let first: Vec<DatasetItem> = (0..3).map(|n| item(&id, n)).collect();
        let added = store.add_dataset_items(&id, &first).await.expect("add three cases");
        assert_eq!(added, 3, "{backend} did not report three cases added");

        let after_first = find_dataset(
            &store.list_datasets(&project).await.expect("list datasets"),
            &id,
            backend,
        );
        assert_eq!(after_first.item_count, 3, "{backend} counted the first add wrong");

        // The count is recomputed rather than incremented for exactly this
        // case: a capture that is retried, or an item edited in place, must not
        // inflate the number the dataset page shows.
        store.add_dataset_items(&id, &first).await.expect("add the same three again");
        let after_repeat = find_dataset(
            &store.list_datasets(&project).await.expect("list datasets"),
            &id,
            backend,
        );
        assert_eq!(
            after_repeat.item_count, 3,
            "{backend} double counted three items that were added twice"
        );
        assert_eq!(
            store.list_dataset_items(&id, 100).await.expect("read the cases").len(),
            3,
            "{backend} stored a second copy of an item id"
        );

        // A repeat mixed with something new still has to land on the true
        // total, which an implementation that only skipped duplicates would
        // also pass, and one that incremented by the batch size would not.
        store
            .add_dataset_items(&id, &[item(&id, 1), item(&id, 9)])
            .await
            .expect("add one repeat and one new case");
        let after_mixed = find_dataset(
            &store.list_datasets(&project).await.expect("list datasets"),
            &id,
            backend,
        );
        assert_eq!(after_mixed.item_count, 4, "{backend} miscounted a mixed add");
    }
}

#[tokio::test]
async fn listing_dataset_items_stops_at_the_limit_and_keeps_the_source_trace() {
    let project = unique("project");
    let id = unique("dataset");
    let items: Vec<DatasetItem> = (0..5).map(|n| item(&id, n)).collect();

    for (backend, store) in backends().await {
        store.put_dataset(&dataset(&id, &project)).await.expect("put the dataset");
        store.add_dataset_items(&id, &items).await.expect("add five cases");

        let page = store.list_dataset_items(&id, 2).await.expect("read a short page");
        assert_eq!(page.len(), 2, "{backend} returned more than the limit asked for");
        // Which two arrive is not specified, and the backends do order them
        // differently, so each one is matched by id rather than by position.
        for got in &page {
            let want = items
                .iter()
                .find(|w| w.item_id == got.item_id)
                .unwrap_or_else(|| panic!("{backend} returned an item nobody added"));
            assert_eq!(got, want, "{backend} altered a case");
        }

        let all = store.list_dataset_items(&id, 100).await.expect("read every case");
        assert_eq!(all.len(), 5, "{backend} lost a case");
        for want in &items {
            let got = all
                .iter()
                .find(|g| g.item_id == want.item_id)
                .unwrap_or_else(|| panic!("{backend} did not return {}", want.item_id));
            // The trace id is the whole point of a captured case: it is how a
            // bug report stays traceable back to the run it came from.
            assert_eq!(
                got.source_trace_id, want.source_trace_id,
                "{backend} lost the source trace of {}",
                want.item_id
            );
            assert_eq!(got, want, "{backend} altered {}", want.item_id);
        }
    }
}

#[tokio::test]
async fn an_experiment_comes_back_whole_including_one_that_never_started() {
    let project = unique("project");
    let dataset_id = unique("dataset");
    let ran_id = unique("experiment");
    let pending_id = unique("experiment");

    let ran = Experiment {
        experiment_id: ran_id.clone(),
        name: "gpt-4o against the refund set".into(),
        dataset_id: dataset_id.clone(),
        status: ExperimentStatus::Succeeded,
        started_at: Some(created()),
        finished_at: Some(created() + Duration::minutes(4)),
        item_count: 42,
        summary: Some(json!({ "exact_match": { "mean": 0.75, "count": 42 } })),
        baseline_id: Some("baseline-7".into()),
        cost: Some(Cost {
            currency: "USD".into(),
            input_cost: Some(0.25),
            output_cost: Some(0.5),
            total_cost: Some(0.75),
            price_book: Some("builtin".into()),
            is_estimated: true,
        }),
        error: None,
        metadata: Some(json!({ "git_sha": "9f1c2ab" })),
        correlation: corr(&project),
    };
    // Created and not started yet, which is the state every experiment passes
    // through and the one a nullable column gets wrong.
    let pending = Experiment {
        experiment_id: pending_id.clone(),
        name: "queued rerun".into(),
        dataset_id: dataset_id.clone(),
        status: ExperimentStatus::Pending,
        started_at: None,
        finished_at: None,
        item_count: 0,
        summary: None,
        baseline_id: None,
        cost: None,
        error: None,
        metadata: None,
        correlation: corr(&project),
    };

    for (backend, store) in backends().await {
        store.put_experiment(&ran).await.expect("put the finished experiment");
        store.put_experiment(&pending).await.expect("put the pending experiment");

        let listed = store.list_experiments(&project).await.expect("list experiments");
        assert_eq!(listed.len(), 2, "{backend} listed the wrong number of experiments");

        assert_eq!(find_experiment(&listed, &ran_id, backend), ran, "{backend} changed the run");
        let got_pending = find_experiment(&listed, &pending_id, backend);
        assert_eq!(got_pending, pending, "{backend} changed the pending experiment");
        assert!(
            got_pending.started_at.is_none(),
            "{backend} invented a start time for an experiment that never started"
        );

        // Newest first, and one that never started has nothing to be newest
        // by, so it sorts last rather than to the top.
        assert_eq!(listed[0].experiment_id, ran_id, "{backend} did not list the newest first");
        assert_eq!(listed[1].experiment_id, pending_id, "{backend} sorted a null start to the top");
    }
}

#[tokio::test]
async fn a_dataset_in_one_project_is_never_listed_under_another() {
    let project_a = unique("project");
    let project_b = unique("project");
    let in_a = unique("dataset");
    let in_b = unique("dataset");

    for (backend, store) in backends().await {
        store.put_dataset(&dataset(&in_a, &project_a)).await.expect("put A");
        store.put_dataset(&dataset(&in_b, &project_b)).await.expect("put B");

        let a = store.list_datasets(&project_a).await.expect("list A");
        let b = store.list_datasets(&project_b).await.expect("list B");

        assert_eq!(a.len(), 1, "{backend} listed more than project A's own dataset");
        assert_eq!(a[0].dataset_id, in_a, "{backend} listed the wrong dataset for A");
        assert_eq!(b.len(), 1, "{backend} listed more than project B's own dataset");
        assert_eq!(b[0].dataset_id, in_b, "{backend} listed the wrong dataset for B");
        assert!(
            !a.iter().any(|d| d.dataset_id == in_b),
            "{backend} leaked project B's dataset into project A"
        );
    }
}

#[tokio::test]
async fn an_experiment_in_one_project_is_never_listed_under_another() {
    let project_a = unique("project");
    let project_b = unique("project");
    let in_a = unique("experiment");
    let in_b = unique("experiment");

    let experiment = |id: &str, project: &str| Experiment {
        experiment_id: id.to_owned(),
        name: "nightly".into(),
        dataset_id: "shared-dataset".into(),
        status: ExperimentStatus::Running,
        started_at: Some(created()),
        finished_at: None,
        item_count: 3,
        summary: None,
        baseline_id: None,
        cost: None,
        error: None,
        metadata: None,
        correlation: corr(project),
    };

    for (backend, store) in backends().await {
        store.put_experiment(&experiment(&in_a, &project_a)).await.expect("put A");
        store.put_experiment(&experiment(&in_b, &project_b)).await.expect("put B");

        let a = store.list_experiments(&project_a).await.expect("list A");
        let b = store.list_experiments(&project_b).await.expect("list B");

        assert_eq!(a.len(), 1, "{backend} listed more than project A's own experiment");
        assert_eq!(a[0].experiment_id, in_a, "{backend} listed the wrong experiment for A");
        assert_eq!(b.len(), 1, "{backend} listed more than project B's own experiment");
        assert_eq!(b[0].experiment_id, in_b, "{backend} listed the wrong experiment for B");
        // A dataset shared between two projects must not make one project's
        // runs visible from the other.
        assert!(
            !a.iter().any(|e| e.experiment_id == in_b),
            "{backend} leaked project B's experiment into project A"
        );
    }
}

#[tokio::test]
async fn a_dataset_id_resolves_to_its_own_project_and_an_unknown_one_to_nothing() {
    let project = unique("project");
    let id = unique("dataset");
    let never_written = unique("dataset");

    for (backend, store) in owner_backends().await {
        // Asked before the row exists. A backend that answers this one with a
        // project has invented an owner, which is the failure that matters most
        // here: it authorises a read of something that is not there.
        assert_eq!(
            store.dataset_project(&id).await.expect("ask before the dataset exists"),
            None,
            "{backend} named an owner for a dataset that had not been created"
        );

        store.put_dataset(&dataset(&id, &project)).await.expect("put the dataset");

        assert_eq!(
            store.dataset_project(&id).await.expect("ask who owns it"),
            Some(project.clone()),
            "{backend} named the wrong owner"
        );
        assert_eq!(
            store.dataset_project(&never_written).await.expect("ask about an unknown id"),
            None,
            "{backend} named an owner for an id nobody wrote"
        );
    }
}

#[tokio::test]
async fn clickhouse_refuses_a_dataset_id_that_two_projects_claim() {
    let Some(store) = clickhouse_store().await else {
        eprintln!("MCPD_TEST_CLICKHOUSE_URL not set, skipping");
        return;
    };
    let project_a = unique("project");
    let project_b = unique("project");
    let shared = unique("dataset");
    let rewritten = unique("dataset");

    // Rewriting a dataset in place is the ordinary case and must still answer.
    // `datasets` is a replacing table, so this leaves two rows under one key
    // until a merge folds them, and a refusal counted by rows rather than by
    // project would fire on it.
    store.put_dataset(&dataset(&rewritten, &project_a)).await.expect("put it once");
    let mut edited = dataset(&rewritten, &project_a);
    edited.name = "Renamed after the first capture".into();
    store.put_dataset(&edited).await.expect("write it again");
    assert_eq!(
        store.dataset_project(&rewritten).await.expect("ask who owns the rewritten dataset"),
        Some(project_a.clone()),
        "a dataset rewritten in its own project should still name that project"
    );

    // Two tenants, one id. ClickHouse sorts `datasets` by (project, dataset_id),
    // so both rows survive every merge and neither is the answer.
    store.put_dataset(&dataset(&shared, &project_a)).await.expect("put A");
    store.put_dataset(&dataset(&shared, &project_b)).await.expect("put B");

    let err = store
        .dataset_project(&shared)
        .await
        .expect_err("an id two projects claim has no owner to authorise against");
    let message = err.to_string();
    assert!(
        message.contains(&shared),
        "the refusal should name the dataset it is about, got: {message}"
    );
    // The caller supplied an id and holds no grant for either tenant, so the
    // refusal must not hand back the names of the projects it found.
    assert!(
        !message.contains(&project_a) && !message.contains(&project_b),
        "the refusal leaked a project name: {message}"
    );
}
