//! Versioned Notes migrations. Runtime startup never upgrades the schema.
use sqlx::{
    Connection, PgConnection,
    migrate::{Migrate, Migrator},
};

type Error = Box<dyn std::error::Error>;
const TABLE: &str = "_fieldnotes_notes_migrations";
const INITIAL_CHECKSUMS: [&str; 3] = [
    "3572891692dca5da451b54befd7406374d98c2f2a298869240b7c9c4022dbdf21f9c21342e8089967d09e012865f654c",
    "e3f4f33180ab23399330a1b2159a98fb8e73a918c2f97b685968132568867cfe60bda29b3dbe91f2576403e80a1f0b44",
    "df399498c4724ce0cc0b429d1450a27f376a02b48cd43f7cff446a53547bbc3a521b38c85e02ba44ac1aab828580c02b",
];

fn migrator() -> Migrator {
    let mut migrations = sqlx::migrate!("./migrations");
    migrations.dangerous_set_table_name(TABLE);
    migrations
}

pub async fn execute(command: &str) -> Result<(), Error> {
    use framework_core::persistence::postgres09::{
        PostgresConnectionPolicy, connect_migration_postgres,
    };
    let pool = connect_migration_postgres(
        &std::env::var("DATABASE_URL")?,
        PostgresConnectionPolicy::from_env(1)?,
    )
    .await?;
    let mut connection = pool.acquire().await?;
    let migrations = migrator();
    match command {
        "--migrate-only" => {
            migrations.run(&mut *connection).await?;
        }
        "--migrate-status" => status(&mut connection, &migrations).await?,
        "--migrate-baseline" => {
            if std::env::var("NOTES_BASELINE_CONFIRM").as_deref() != Ok("reviewed-initial-schema") {
                return Err("Baseline requires NOTES_BASELINE_CONFIRM=reviewed-initial-schema after a schema review; see migrations/README.md".into());
            }
            connection.lock().await?;
            let result = baseline(&mut connection, &migrations).await;
            let unlocked = connection.unlock().await;
            result?;
            unlocked?;
        }
        _ => return Err("unknown migration command".into()),
    }
    Ok(())
}

async fn status(connection: &mut PgConnection, migrations: &Migrator) -> Result<(), Error> {
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('public._fieldnotes_notes_migrations') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await?;
    let applied = if exists {
        if connection.dirty_version(TABLE).await?.is_some() {
            return Err("failed migration history".into());
        }
        connection.list_applied_migrations(TABLE).await?
    } else {
        Vec::new()
    };
    for row in &applied {
        if !migrations
            .iter()
            .any(|migration| migration.version == row.version && migration.checksum == row.checksum)
        {
            return Err(format!("unknown or changed applied migration {}", row.version).into());
        }
    }
    for migration in migrations.iter() {
        let state = if applied.iter().any(|row| row.version == migration.version) {
            "applied"
        } else {
            "pending"
        };
        println!("{} {} {}", migration.version, state, migration.description);
    }
    Ok(())
}

async fn baseline(connection: &mut PgConnection, migrations: &Migrator) -> Result<(), Error> {
    for (index, checksum) in INITIAL_CHECKSUMS.iter().enumerate() {
        let migration = migrations
            .iter()
            .find(|migration| migration.version == index as i64 + 1)
            .ok_or("missing initial migration")?;
        let actual: String = migration
            .checksum
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != *checksum {
            return Err("initial migration checksum changed; baseline refused".into());
        }
    }
    let mut transaction = connection.begin().await?;
    // The operator must compare the full existing schema with the three initial SQL files.
    // These guards reject common partial/unsafe legacy installations; they are not a drift audit.
    let valid: bool = sqlx::query_scalar(r#"
        SELECT (SELECT count(*) = 4 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
          WHERE n.nspname='public' AND c.relname IN ('fieldnotes_notes','fieldnotes_outbox','fieldnotes_events','fieldnotes_attachments')
          AND c.relkind='r' AND c.relrowsecurity AND c.relforcerowsecurity)
        AND EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema='public' AND table_name='fieldnotes_notes' AND column_name='cache_version' AND is_nullable='NO' AND data_type='bigint')
        AND EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid='public.fieldnotes_notes'::regclass AND tgname='fieldnotes_note_cache_version' AND tgenabled='O')
        AND (SELECT count(*) = 7 FROM pg_policies WHERE schemaname='public' AND policyname IN ('notes_tenant','outbox_tenant','events_tenant','outbox_dispatch','attachment_tenant','attachment_worker_read','attachment_worker_delete'))
        AND NOT has_table_privilege('fieldnotes_worker','public.fieldnotes_notes','SELECT')
        AND has_table_privilege('fieldnotes_api','public.fieldnotes_notes','SELECT')
    "#).fetch_one(&mut *transaction).await?;
    if !valid {
        return Err("legacy schema guard failed; baseline refused".into());
    }
    (*transaction).ensure_migrations_table(TABLE).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _fieldnotes_notes_migrations")
        .fetch_one(&mut *transaction)
        .await?;
    if count != 0 {
        return Err("migration history already exists; baseline refused".into());
    }
    for migration in migrations.iter().filter(|migration| migration.version <= 3) {
        sqlx::query("INSERT INTO _fieldnotes_notes_migrations(version,description,success,checksum,execution_time) VALUES($1,$2,true,$3,0)")
            .bind(migration.version).bind(migration.description.as_ref()).bind(migration.checksum.as_ref()).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    println!("Recorded reviewed initial Notes migrations 1–3; no application DDL or data changed.");
    Ok(())
}
